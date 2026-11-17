"""Compare two completed synthetic localhost qualification reports; no benchmark is run."""

import json
import os
import re
import stat
import statistics
import sys

CASES = ("literal", "rewrite", "selection", "preview")
MAX_REPORT_BYTES = 128 * 1024
MAX_TIME_US = 120_000_000
MAX_COUNT = 1_000_000
MAX_OGG_BYTES = 16 * 1024 * 1024
TOP_KEYS = {"schema", "scope", "repeats", "rows"}
ROW_KEYS = {"case", "repeat", "pcm_samples", "pcm_sha256", "wire_responses_sha256",
            "rendered_sha256", "request_count", "response_count", "ogg_bytes",
            "first_response_us", "session_us"}
SEMANTIC_KEYS = ROW_KEYS - {"first_response_us", "session_us"}
HASH = re.compile(r"[0-9a-f]{64}\Z")


class InvalidReport(ValueError):
    pass


def _pairs(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise InvalidReport()
        result[key] = value
    return result


def _reject_number(_value):
    raise InvalidReport()


def _integer(value, low, high):
    return type(value) is int and low <= value <= high


def validate_report(report):
    if (type(report) is not dict or set(report) != TOP_KEYS
            or report['schema'] != 'rambler-native-qualification-v1'
            or report['scope'] != 'synthetic-localhost-only'
            or not _integer(report['repeats'], 1, 10)
            or type(report['rows']) is not list
            or len(report['rows']) != report['repeats'] * len(CASES)):
        raise InvalidReport()
    for index, row in enumerate(report['rows']):
        if type(row) is not dict or set(row) != ROW_KEYS:
            raise InvalidReport()
        if (row['case'] != CASES[index % len(CASES)]
                or not _integer(row['repeat'], 0, 9)
                or row['repeat'] != index // len(CASES)
                or not _integer(row['pcm_samples'], 1, 160_000)
                or not _integer(row['request_count'], 1, MAX_COUNT)
                or not _integer(row['response_count'], 1, MAX_COUNT)
                or not _integer(row['ogg_bytes'], 1, MAX_OGG_BYTES)
                or not _integer(row['first_response_us'], 0, MAX_TIME_US)
                or not _integer(row['session_us'], 0, MAX_TIME_US)
                or row['first_response_us'] > row['session_us']):
            raise InvalidReport()
        for key in ('pcm_sha256', 'wire_responses_sha256', 'rendered_sha256'):
            if type(row[key]) is not str or not HASH.fullmatch(row[key]):
                raise InvalidReport()
    return report


def _same_file(a, b):
    return (a.st_dev, a.st_ino, a.st_size, a.st_mtime_ns) == (
        b.st_dev, b.st_ino, b.st_size, b.st_mtime_ns)


def read_report(path):
    """Bound reads and reject links, special files and observed replacement/modification.

    These checks are best effort, not a defense against a malicious same-user writer.
    """
    try:
        before = os.lstat(path)
        if not stat.S_ISREG(before.st_mode) or before.st_size > MAX_REPORT_BYTES:
            raise InvalidReport()
        flags = os.O_RDONLY | getattr(os, 'O_BINARY', 0) | getattr(os, 'O_NOFOLLOW', 0) | getattr(os, 'O_NONBLOCK', 0)
        fd = os.open(path, flags)
        with os.fdopen(fd, 'rb') as stream:
            opened = os.fstat(stream.fileno())
            if not stat.S_ISREG(opened.st_mode) or not _same_file(before, opened):
                raise InvalidReport()
            data = stream.read(MAX_REPORT_BYTES + 1)
            after = os.fstat(stream.fileno())
        current = os.lstat(path)
        if (len(data) > MAX_REPORT_BYTES or len(data) != opened.st_size
                or not stat.S_ISREG(current.st_mode)
                or not _same_file(opened, after) or not _same_file(after, current)):
            raise InvalidReport()
        report = json.loads(data.decode('utf-8'), object_pairs_hook=_pairs,
                            parse_float=_reject_number, parse_constant=_reject_number)
        return validate_report(report)
    except (OSError, ValueError, UnicodeError, RecursionError, OverflowError):
        raise InvalidReport() from None


def compare_reports(baseline, candidate):
    validate_report(baseline)
    validate_report(candidate)
    if (baseline['repeats'] != candidate['repeats'] or any(
            any(a[key] != b[key] for key in SEMANTIC_KEYS)
            for a, b in zip(baseline['rows'], candidate['rows']))):
        return {'status': 'refused', 'reason': 'semantic_mismatch'}
    cases = []
    for case_index, case in enumerate(CASES):
        result = {'case': case, 'sample_count': baseline['repeats']}
        for metric in ('first_response_us', 'session_us'):
            deltas = [candidate['rows'][i * len(CASES) + case_index][metric]
                      - baseline['rows'][i * len(CASES) + case_index][metric]
                      for i in range(baseline['repeats'])]
            result[metric + '_delta'] = {'samples': deltas, 'median': statistics.median(deltas),
                                         'min': min(deltas), 'max': max(deltas)}
        cases.append(result)
    return {'status': 'accepted', 'delta_definition': 'candidate minus baseline, microseconds',
            'interpretation': 'Descriptive local synthetic observations only; execution order and environment can bias results. No speedup or real-service claim.',
            'cases': cases}


def main(argv=None):
    args = sys.argv[1:] if argv is None else argv
    if len(args) != 2:
        result = {'status': 'refused', 'reason': 'invalid_arguments'}
    else:
        try:
            result = compare_reports(read_report(args[0]), read_report(args[1]))
        except InvalidReport:
            result = {'status': 'refused', 'reason': 'invalid_report'}
    print(json.dumps(result, separators=(',', ':'), allow_nan=False))
    return 0 if result['status'] == 'accepted' else 1


if __name__ == '__main__':
    sys.exit(main())
