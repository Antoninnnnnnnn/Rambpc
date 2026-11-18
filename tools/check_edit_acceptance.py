"""Check predeclared edit expectations against independent, local observations."""

import json
import os
from pathlib import Path
import re
import stat
import sys

EXPECTATIONS = 'rambpc-edit-expectations-v1'
OBSERVATIONS = 'rambpc-edit-observations-v1'
RESULT = 'rambpc-edit-acceptance-result-v1'
MAX_BYTES = 1024 * 1024
MAX_CASES = 100
MAX_WINDOW_UNITS = 65536
IDENTIFIER = re.compile(r'[A-Za-z0-9][A-Za-z0-9_-]{0,63}')
SUPPORTED_INTENTS = (None, 2, 3)
EXIT_CODES = {'conforming': 0, 'mismatch': 1, 'inconclusive': 2, 'invalid': 3}


class InvalidInput(ValueError):
    pass


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise InvalidInput()
        result[key] = value
    return result


def _reject_number(_value):
    raise InvalidInput()


def _parse_int(token):
    if len(token) > 11:
        raise InvalidInput()
    return int(token)


def _keys(value, *keys):
    if type(value) is not dict or value.keys() != set(keys):
        raise InvalidInput()


def _identifier(value):
    if type(value) is not str or not IDENTIFIER.fullmatch(value):
        raise InvalidInput()


def _choice(value, *choices):
    if type(value) is not str or value not in choices:
        raise InvalidInput()
    return value


def _outcome(value):
    if type(value) is not dict:
        raise InvalidInput()
    if _choice(value.get('kind'), 'no_edit', 'edit') == 'no_edit':
        _keys(value, 'kind')
        return
    _keys(value, 'kind', 'window', 'selection_utf16')
    text = value['window']
    if type(text) is not str or len(text) > MAX_WINDOW_UNITS:
        raise InvalidInput()
    encoded = text.encode('utf-16-le')
    units = len(encoded) // 2
    if units > MAX_WINDOW_UNITS:
        raise InvalidInput()
    selection = value['selection_utf16']
    if selection is None:
        return
    if (type(selection) is not list or len(selection) != 2
            or any(type(offset) is not int for offset in selection)
            or not 0 <= selection[0] <= selection[1] <= units):
        raise InvalidInput()
    # At a scalar boundary, the following UTF-16 code unit cannot be a low surrogate.
    if any(offset < units and 0xDC <= encoded[2 * offset + 1] <= 0xDF
           for offset in selection):
        raise InvalidInput()


def _validate(document, schema):
    expectations = schema == EXPECTATIONS
    _keys(document, 'schema', 'artifact_id', 'provenance', 'cases')
    _choice(document['schema'], schema)
    _identifier(document['artifact_id'])
    provenance = document['provenance']
    _keys(provenance, 'corpus_id', 'run_id', 'candidate_id', 'policy')
    _choice(provenance['policy'], 'strict')
    for key in ('corpus_id', 'run_id', 'candidate_id'):
        _identifier(provenance[key])

    rows = document['cases']
    minimum_rows = 1 if expectations else 0
    if type(rows) is not list or not minimum_rows <= len(rows) <= MAX_CASES:
        raise InvalidInput()
    cases = {}
    for row in rows:
        if type(row) is not dict:
            raise InvalidInput()
        if expectations:
            _keys(row, 'case_id', 'input_id', 'intent', 'outcome')
        elif _choice(row.get('status'), 'observed', 'unqualified') == 'unqualified':
            _keys(row, 'case_id', 'input_id', 'status')
        else:
            _keys(row, 'case_id', 'input_id', 'status', 'intent', 'outcome')
        _identifier(row['case_id'])
        _identifier(row['input_id'])
        if row['case_id'] in cases:
            raise InvalidInput()
        cases[row['case_id']] = row
        if not expectations and row['status'] == 'unqualified':
            continue
        intent = row['intent']
        if intent is not None and (type(intent) is not int or not -(2**31) <= intent < 2**31):
            raise InvalidInput()
        _outcome(row['outcome'])
        if expectations and (intent not in SUPPORTED_INTENTS or
                             (row['outcome']['kind'] == 'edit' and intent != 2)):
            raise InvalidInput()
    return cases


def _file_state(info):
    return info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns


def _read(path, schema):
    # Best-effort checks for a local regular file, not protection against a hostile writer.
    initial = path.lstat()
    if not stat.S_ISREG(initial.st_mode) or initial.st_size > MAX_BYTES:
        raise InvalidInput()
    flags = os.O_RDONLY | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0)
    with os.fdopen(os.open(path, flags | getattr(os, 'O_BINARY', 0)), 'rb') as stream:
        opened = os.fstat(stream.fileno())
        if not stat.S_ISREG(opened.st_mode) or _file_state(opened) != _file_state(initial):
            raise InvalidInput()
        raw = stream.read(MAX_BYTES + 1)
        if (len(raw) > MAX_BYTES or len(raw) != opened.st_size
                or _file_state(os.fstat(stream.fileno())) != _file_state(opened)
                or _file_state(path.lstat()) != _file_state(opened)):
            raise InvalidInput()
    document = json.loads(raw.decode('utf-8'), object_pairs_hook=_pairs,
                          parse_int=_parse_int, parse_float=_reject_number,
                          parse_constant=_reject_number)
    return document, _validate(document, schema)


def _compare(expectations, observations):
    declaration, expected = expectations
    evidence, observed = observations
    counts = dict.fromkeys(('conforming_no_edit', 'conforming_edit', 'mismatch',
                            'missing', 'unqualified', 'unexpected'), 0)
    if (declaration['provenance'] != evidence['provenance']
            or declaration['artifact_id'] == evidence['artifact_id']):
        counts['unqualified'] = len(expected)
    else:
        counts['unexpected'] = len(observed.keys() - expected.keys())
        for case_id, wanted in expected.items():
            actual = observed.get(case_id)
            if actual is None:
                counts['missing'] += 1
            elif (actual['input_id'] != wanted['input_id'] or actual['status'] != 'observed'
                  or actual['intent'] not in SUPPORTED_INTENTS):
                counts['unqualified'] += 1
            elif actual['intent'] != wanted['intent'] or actual['outcome'] != wanted['outcome']:
                counts['mismatch'] += 1
            else:
                counts['conforming_' + wanted['outcome']['kind']] += 1
    if counts['mismatch']:
        verdict = 'mismatch'
    elif counts['missing'] or counts['unqualified'] or counts['unexpected']:
        verdict = 'inconclusive'
    else:
        verdict = 'conforming'
    return verdict, {'counts': {'expected': len(expected), **counts}}


def main(argv=None):
    args = sys.argv[1:] if argv is None else argv
    if args == ['--help']:
        print('Usage: check_edit_acceptance.py EXPECTATIONS_JSON OBSERVATIONS_JSON')
        return 0
    if len(args) != 2:
        verdict, details = 'invalid', {'reason': 'invalid_arguments'}
    else:
        role = 'expectations'
        try:
            expectations = _read(Path(args[0]), EXPECTATIONS)
            role = 'observations'
            observations = _read(Path(args[1]), OBSERVATIONS)
        except (OSError, ValueError, RecursionError, OverflowError):
            verdict, details = 'invalid', {'reason': 'invalid_' + role}
        else:
            verdict, details = _compare(expectations, observations)
    print(json.dumps({'schema': RESULT, 'scope': 'offline-strict-proposal-only',
                      'verdict': verdict, **details}, separators=(',', ':')))
    return EXIT_CODES[verdict]


if __name__ == '__main__':
    raise SystemExit(main())
