"""Create a NEW, allowlisted copy of a validate_corpus JSON report, offline.

Inspect before publication. This tool does NOT prove that private speech, editing
context, local case labels, or machine metadata are publishable. Semantic text is
hashed by default. --include-text is an explicit assertion that the corpus text
is controlled and has been reviewed; it never enables raw payloads, unknown-field
values, result/session identifiers, or arbitrary report keys. Hashes also retain
correlation and can reveal low-entropy text through guessing.

Input is never changed, output must not exist, and no credentials/network/app
modules are loaded. Original protobuf bytes are represented only by hashes,
lengths and top-level field numbers/wire types. No unknown payload is interpreted
as trusted semantic text.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
from pathlib import Path
import re

MAX_REPORT_BYTES = 32 * 1024 * 1024
MAX_PAYLOAD_BYTES = 8 * 1024 * 1024
MAX_FIELDS = 100_000
_OMIT = object()
_HASH = re.compile(r'[0-9a-fA-F]{64}\Z')
_GIT = re.compile(r'[0-9a-fA-F]{40}\Z')
_SOURCE_FILES = frozenset(('__init__.py', 'audio.py', 'cli.py', 'credentials.py',
                           'encoding.py', 'engine.py', 'protocol.py', 'results.py',
                           'transport.py', 'wire.py'))
_LIBRARIES = frozenset(('rambpc-engine', 'grpcio', 'av'))
_ENUMS = {
    'transport': {'success', 'failed_or_unverified'},
    'rendering': {'unique', 'ambiguous', 'error'},
    'pacing': {'wav_20ms_realtime', 'unpaced', 'not_verified'},
    'source_kind': {'pcm16le', 'ogg'},
    'outcome': {'success', 'failed'},
    'grpc_code': {'OK', 'CANCELLED', 'UNKNOWN', 'INVALID_ARGUMENT',
                  'DEADLINE_EXCEEDED', 'NOT_FOUND', 'ALREADY_EXISTS',
                  'PERMISSION_DENIED', 'RESOURCE_EXHAUSTED', 'FAILED_PRECONDITION',
                  'ABORTED', 'OUT_OF_RANGE', 'UNIMPLEMENTED', 'INTERNAL',
                  'UNAVAILABLE', 'DATA_LOSS', 'UNAUTHENTICATED'},
    'error_class': {'ValueError', 'TypeError', 'TimeoutError', 'RuntimeError',
                    'LookupError', 'ImportError', 'FileNotFoundError',
                    'AioRpcError', 'CancelledError', 'IncompleteSessionError',
                    'S3SessionError', 'ContextConflictError', 'AmbiguousResultError'},
}
_WIRE = {'raw_summary': ('raw', 'bytes_summary'),
         'unknown_field_summaries': ('unknown_fields', 'unknown_fields')}
_SCHEMAS = {
    'context': {**_WIRE, 'before': 'text', 'selected': 'text', 'after': 'text',
                'cursor_aware': 'bool'},
    'voice': {**_WIRE, 'transcript': 'text', 'alternative_text': 'text',
              'edited_text': 'text', 'edited_context': 'context', 'intent': 'int',
              'active_field': 'int'},
    'hypothesis': {**_WIRE, 'text': 'text', 'confidence': 'number', 'voice_edit': 'voice'},
    'partial': {**_WIRE, 'text': 'text', 'confidence': 'number'},
    'hypotheses': {**_WIRE, 'hypotheses': 'hypothesis[]'},
    'partials': {**_WIRE, 'partials': 'partial[]'},
    'recognition': {**_WIRE, 'state': 'int', 'hypothesis_result': 'hypotheses',
                    'partial_result': 'partials'},
    'speech': {**_WIRE, 'event': 'int'},
    'result': {**_WIRE, 'recognition': 'recognition', 'speech': 'speech',
               'alternative_text': 'text', 'active_field': 'int',
               'active_payload_summary': ('active_raw', 'bytes_summary')},
    'response': {**_WIRE, 'status': 'int', 'error_code': 'int',
                 # Server diagnostics are never covered by the controlled-text opt-in.
                 'error_message_summary': ('error_message', 'hashed_text'),
                 'result': 'result'},
    'metrics': {name: 'number' for name in (
        'requests_produced', 'request_protobuf_bytes', 'responses_received',
        'response_protobuf_bytes', 'elapsed_seconds', 'first_response_seconds',
        'eof_seconds')},
    'timing': {'collection_seconds': 'number', 'engine_metrics_at_eof': 'metrics'},
    'audio': {'file_sha256': 'hash', 'source_sha256': 'hash',
              'source_kind': 'enum:source_kind', 'duration_s': 'number'},
    'consumed': {'bytes': 'int', 'sha256': 'hash', 'complete': 'bool'},
    'wer': {'errors': 'int', 'reference_words': 'int', 'rate': 'number'},
    'proposal': {'snapshot': 'context', 'replacement_text': 'text',
                 'replacement_context': 'context'},
    'cpu': {'model': 'label', 'processor': 'label', 'machine': 'label',
            'architecture': 'label', 'logical_cores': 'int', 'physical_cores': 'int'},
    'versions': {'python': 'label', 'os': 'label', 'cpu': 'cpu', 'processor': 'label',
                 'machine': 'label', 'libraries': 'libraries', 'git_head': 'git_hash',
                 'rambler_source_sha256': 'source_hashes', 'validator_sha256': 'hash'},
    'summary': {'requested': 'int', 'completed_successfully': 'int', 'failed': 'int',
                'not_run': 'int', 'status': 'enum:outcome'},
    'case': {'id': 'label', 'transport': 'enum:transport', 'audio': 'audio',
             'consumed_source': 'consumed', 'sent_ogg': 'consumed',
             'effective_pacing': 'enum:pacing', 'timing': 'timing',
             'final_update_count': 'int', 'rendering': 'enum:rendering',
             'final_updates': 'response[]', 'context': 'context',
             'rendered_text': 'text', 'wer': 'wer', 'required_terms': 'terms',
             'context_conflict_detected': 'bool', 'revision_conflict_detected': 'bool',
             'replacement_verified': 'bool', 'replacement_proposal': 'proposal',
             'error_class': 'enum:error_class', 'render_error_class': 'enum:error_class',
             'grpc_code': 'enum:grpc_code'},
    'report': {'versions': 'versions', 'cpu': 'cpu', 'requested_realtime': 'bool',
               'source_include_results': ('include_results', 'bool'),
               'summary': 'summary', 'cases': 'case[]'},
}


def _digest(data: bytes) -> dict:
    return {'sha256': hashlib.sha256(data).hexdigest(), 'bytes': len(data)}


def _varint(data: bytes, offset: int) -> tuple[int, int]:
    value = 0
    for index in range(10):
        if offset == len(data):
            raise ValueError('truncated protobuf')
        byte = data[offset]
        offset += 1
        if index == 9 and byte > 1:
            raise ValueError('invalid protobuf varint')
        value |= (byte & 127) << (index * 7)
        if byte < 128:
            return value, offset
    raise ValueError('invalid protobuf varint')


def _field_numbers(data: bytes) -> list[dict]:
    """Inspect only framing, never emit protobuf values or decode unknown text."""
    result = []
    offset = 0
    while offset < len(data):
        if len(result) >= MAX_FIELDS:
            raise ValueError('too many protobuf fields')
        key, offset = _varint(data, offset)
        number, wire = key >> 3, key & 7
        if not 1 <= number < 2**29:
            raise ValueError('invalid protobuf field')
        if wire == 0:
            _, offset = _varint(data, offset)
        elif wire in (1, 5):
            offset += 8 if wire == 1 else 4
        elif wire == 2:
            size, offset = _varint(data, offset)
            offset += size
        else:
            raise ValueError('unsupported protobuf wire type')
        if offset > len(data):
            raise ValueError('truncated protobuf')
        result.append({'number': number, 'wire_type': wire})
    return result


def _bytes_summary(value):
    if not isinstance(value, dict) or not isinstance(value.get('hex'), str):
        return _OMIT
    encoded = value['hex']
    if len(encoded) > 2 * MAX_PAYLOAD_BYTES:
        raise ValueError('protobuf payload exceeds limit')
    try:
        data = bytes.fromhex(encoded)
    except ValueError:
        return {'withheld': True, 'valid_hex': False}
    summary = _digest(data)
    try:
        summary['top_level_fields'] = _field_numbers(data)
    except ValueError:
        summary['protobuf_framing_valid'] = False
    return summary


def _text(value, include_text):
    if not isinstance(value, str):
        return _OMIT
    encoded = value.encode('utf-8', errors='strict')
    return value if include_text else _digest(encoded)


def _unknown_fields(value):
    if not isinstance(value, list):
        return _OMIT
    if len(value) > MAX_FIELDS:
        raise ValueError('too many unknown fields')
    result = []
    for item in value:
        if not isinstance(item, dict):
            continue
        number, wire = item.get('number'), item.get('wire_type')
        if type(number) is not int or not 1 <= number < 2**29 or type(wire) is not int or not 0 <= wire <= 5:
            continue
        summary = {'number': number, 'wire_type': wire, 'value_withheld': True}
        payload = _bytes_summary(item.get('value'))
        if payload is not _OMIT:
            summary['payload_summary'] = payload
        result.append(summary)
    return result


def _value(value, kind, include_text):
    if value is None:
        return None
    if kind.endswith('[]'):
        if not isinstance(value, list):
            return _OMIT
        if len(value) > MAX_FIELDS:
            raise ValueError('too many report items')
        return [out for item in value if (out := _project(item, kind[:-2], include_text)) is not _OMIT]
    if kind in _SCHEMAS:
        return _project(value, kind, include_text)
    if kind == 'text':
        return _text(value, include_text)
    if kind == 'hashed_text':
        return _text(value, False)
    if kind == 'bytes_summary':
        return _bytes_summary(value)
    if kind == 'unknown_fields':
        return _unknown_fields(value)
    if kind == 'bool':
        return value if type(value) is bool else _OMIT
    if kind == 'int':
        return value if type(value) is int else _OMIT
    if kind == 'number':
        if type(value) in (int, float):
            if type(value) is float and not math.isfinite(value):
                return {'nonfinite_float': 'nan' if math.isnan(value) else ('inf' if value > 0 else '-inf')}
            return value
        if isinstance(value, dict) and value.get('nonfinite_float') in ('nan', 'inf', '-inf'):
            return {'nonfinite_float': value['nonfinite_float']}
        return _OMIT
    if kind in ('hash', 'git_hash'):
        pattern = _HASH if kind == 'hash' else _GIT
        return value.lower() if isinstance(value, str) and pattern.fullmatch(value) else _OMIT
    if kind == 'label':
        # Locally assigned case labels and runtime/machine version metadata only.
        return value if isinstance(value, str) and len(value) <= 1024 else _OMIT
    if kind.startswith('enum:'):
        return value if isinstance(value, str) and value in _ENUMS[kind[5:]] else _OMIT
    if kind in ('libraries', 'source_hashes'):
        if not isinstance(value, dict):
            return _OMIT
        allowed = _LIBRARIES if kind == 'libraries' else _SOURCE_FILES
        return {key: out for key in sorted(allowed) if key in value
                and (out := _value(value[key], 'label' if kind == 'libraries' else 'hash', include_text)) is not _OMIT}
    if kind == 'terms':
        if not isinstance(value, dict):
            return _OMIT
        return [{'term': _text(term, include_text), 'present': present}
                for term, present in value.items() if isinstance(term, str) and type(present) is bool]
    raise ValueError('unknown sanitizer schema')


def _project(value, schema, include_text):
    if not isinstance(value, dict):
        return _OMIT
    result = {}
    for output_name, spec in _SCHEMAS[schema].items():
        source_name, kind = spec if isinstance(spec, tuple) else (output_name, spec)
        if source_name in value:
            out = _value(value[source_name], kind, include_text)
            if out is not _OMIT:
                result[output_name] = out
    return result


def sanitize_report(report: dict, *, include_text: bool = False) -> dict:
    """Project the known validator schema; unknown keys/values are never copied."""
    if not isinstance(report, dict) or not isinstance(report.get('cases'), list):
        raise ValueError('expected a corpus validation report')
    result = _project(report, 'report', include_text)
    result.update({
        'schema_version': 1,
        'report_kind': 'sanitized_validate_corpus_report',
        'privacy_review_required': True,
        'text_included': bool(include_text),
        'publication_warning': 'Inspect before publishing. Controlled-text opt-in is not proof that private speech, context, local labels or machine metadata are publishable. Hashes retain correlation and may be guessed.',
        'withheld': ['raw_protobuf_bytes', 'unknown_field_values', 'service_result_ids',
                     'session_ids', 'expected_revision', 'unrecognized_report_keys'],
    })
    return result


def _reject_constant(_value):
    raise ValueError('non-finite JSON constants are not allowed')


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON keys are not allowed')
        result[key] = value
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True,
                        help='NEW file only; the original is never changed')
    parser.add_argument('--include-text', action='store_true',
                        help='explicitly assert the corpus semantic text is controlled and reviewed; inspect before publication')
    args = parser.parse_args(argv)
    if args.input.resolve() == args.output.resolve() or args.output.exists():
        raise FileExistsError('output must be a new file distinct from input')
    with args.input.open('rb') as source:
        original = source.read(MAX_REPORT_BYTES + 1)
    if len(original) > MAX_REPORT_BYTES:
        raise ValueError('input report exceeds limit')
    report = json.loads(original.decode('utf-8'), parse_constant=_reject_constant,
                        object_pairs_hook=_unique_object)
    result = sanitize_report(report, include_text=args.include_text)
    result['source_report_summary'] = _digest(original)
    result['sanitizer_source_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    # Fully validate serialization before creating any output, including Unicode.
    output = json.dumps(result, ensure_ascii=False, indent=2, allow_nan=False) + '\n'
    output.encode('utf-8', errors='strict')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open('x', encoding='utf-8', newline='\n') as target:
        target.write(output)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
