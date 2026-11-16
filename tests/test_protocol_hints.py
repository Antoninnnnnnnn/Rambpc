"""Offline request vectors; no APK execution, credentials or network.

Two golden initial requests were captured from a5fa05c before changing protocol.py.
The small reference encoder is independent of rambler.wire. These tests establish
wire assembly only, not hint persistence or server-side effects.
"""

import asyncio
from dataclasses import FrozenInstanceError, fields, replace

import pytest

from rambler.engine import RamblerEngine
from rambler.protocol import (
    MAX_HINTS_PER_CATEGORY,
    MAX_HINT_UTF8_BYTES,
    MAX_SECONDARY_LANGUAGES,
    MAX_SECONDARY_LANGUAGE_UTF8_BYTES,
    EditingContext,
    ProtocolConfig,
    context_request,
    initial_request,
)
from rambler.wire import Field, parse_fields


GOLDEN_DEFAULT = bytes.fromhex(
    "0a156265796f6e642d6132612d7265636f676e697a65721001"
    "c2888f010912070a0566722d4652e28e8f010b1500007a46180a20014800"
    "82c78f0113120667626f6172644206507974686f6ed00101"
    "a2e68f013a0a02200028016801a00101c80201920329"
    "67626f6172645f67656d696e695f76335f73747265616d696e675f766f6963655f656469745f6d756c"
    "a2bfb7710308f403"
)
GOLDEN_CUSTOM = bytes.fromhex(
    "0a156265796f6e642d6132612d7265636f676e697a65721001"
    "c2888f010912070a05656e2d4742e28e8f010c1500803b47182a200248b960"
    "82c78f011c12106d792d6e61746976652d636c69656e7442054c696e7578d00101"
    "a2e68f011f0a02200028016801a00101c8020192030e63616c6c65722d726f7574696e67"
    "a2bfb7710308f403"
)
CUSTOM = ProtocolConfig('en-GB', 'my-native-client', 48000, 42, 2, 'caller-routing', 'Linux')


def uint(value):
    result = bytearray()
    while value > 127:
        result.append((value % 128) + 128)
        value //= 128
    result.append(value)
    return bytes(result)


def blob(number, value):
    if isinstance(value, str):
        value = value.encode('utf-8')
    return uint(number * 8 + 2) + uint(len(value)) + value


def input_boxes(payload):
    return [item.value for item in parse_fields(payload) if item.number == 1203]


@pytest.mark.parametrize('config,elapsed,expected', [
    (ProtocolConfig(), 0, GOLDEN_DEFAULT),
    (CUSTOM, 12345, GOLDEN_CUSTOM),
])
@pytest.mark.parametrize('explicit_empty', [False, True])
def test_two_pre_edit_golden_requests_are_byte_identical(config, elapsed, expected, explicit_empty):
    if explicit_empty:
        config = replace(config, bias_terms=(), preferences=(), secondary_languages=())
    assert initial_request(config, elapsed_ms=elapsed) == expected
    assert input_boxes(expected) == []


@pytest.mark.parametrize('bias,preferences,inner_hex', [
    (('A',), (), '1a0141'),
    ((), ('P',), '2a0150'),
    (('',), (), '1a00'),
    ((), ('',), '2a00'),
    (('A', '', 'A'), ('P', '', 'P'), '1a01411a001a01412a01502a002a0150'),
    (('é', 'é', '𝄞', 'é'), ('  brut\n', '東京', ''),
     '1a02c3a91a0365cc811a04f09d849e1a02c3a92a072020627275740a2a06e69db1e4baac2a00'),
])
def test_hints_exact_wire_unicode_order_duplicates_and_empty_strings(bias, preferences, inner_hex):
    inner = bytes.fromhex(inner_hex)
    config = ProtocolConfig(bias_terms=bias, preferences=preferences)
    payload = initial_request(config)
    assert payload == GOLDEN_DEFAULT + blob(1203, inner)
    assert input_boxes(payload) == [inner]
    assert parse_fields(inner) == (
        [Field(3, 2, value.encode('utf-8')) for value in bias]
        + [Field(5, 2, value.encode('utf-8')) for value in preferences]
    )
    # No premature context (fields1/4), instrument flags or other changed fields.
    assert parse_fields(payload)[:-1] == parse_fields(GOLDEN_DEFAULT)
    assert all(item.number in (3, 5) and item.wire_type == 2 for item in parse_fields(inner))


def test_new_fields_are_appended_frozen_immutable_and_hidden_from_repr():
    names = ('bias_terms', 'preferences', 'secondary_languages')
    assert tuple(item.name for item in fields(ProtocolConfig))[-3:] == names
    config = ProtocolConfig(bias_terms=('private-contact',), preferences=('private-preference',),
                            secondary_languages=('private-locale',))
    assert repr(config) == repr(ProtocolConfig())
    assert all(item.repr is False for item in fields(ProtocolConfig) if item.name in names)
    for name in names:
        assert getattr(ProtocolConfig(), name) == ()
        assert isinstance(getattr(config, name), tuple)
        with pytest.raises(FrozenInstanceError):
            setattr(config, name, ('replacement',))
        with pytest.raises(TypeError):
            getattr(config, name)[0] = 'replacement'
    other = replace(config, bias_terms=('different',))
    assert config.bias_terms == ('private-contact',) and other.bias_terms == ('different',)
    assert ProtocolConfig().bias_terms == ProtocolConfig().preferences == ()


@pytest.mark.parametrize('name', ['bias_terms', 'preferences', 'secondary_languages'])
@pytest.mark.parametrize('invalid', [None, [], ['text'], 'text', b'text', 1, True, {'text'}, {'a': 'b'}])
def test_only_tuples_are_accepted(name, invalid):
    with pytest.raises(ValueError, match=name):
        ProtocolConfig(**{name: invalid})


@pytest.mark.parametrize('name', ['bias_terms', 'preferences', 'secondary_languages'])
def test_generator_is_rejected_without_consuming_it(name):
    def unbounded():
        pytest.fail('must not consume a caller iterator')
        yield 'never'
    values = unbounded()
    try:
        with pytest.raises(ValueError, match='tuple'):
            ProtocolConfig(**{name: values})
    finally:
        values.close()


@pytest.mark.parametrize('name', ['bias_terms', 'preferences', 'secondary_languages'])
@pytest.mark.parametrize('invalid', [None, 1, True, b'text', [], ('nested',)])
def test_tuple_members_must_be_strings(name, invalid):
    with pytest.raises(ValueError, match=name):
        ProtocolConfig(**{name: ('valid', invalid)})


@pytest.mark.parametrize('name', ['bias_terms', 'preferences', 'secondary_languages'])
@pytest.mark.parametrize('invalid', ['\ud800', '\udfff', 'prefix\ud800suffix'])
def test_surrogates_are_rejected_without_echoing_hint_text(name, invalid):
    with pytest.raises(ValueError, match='UTF-8') as caught:
        ProtocolConfig(**{name: (invalid,)})
    assert invalid not in str(caught.value)


@pytest.mark.parametrize('name', ['bias_terms', 'preferences'])
def test_hint_count_boundary_and_empty_entries_are_not_dropped(name):
    assert MAX_HINTS_PER_CATEGORY == 1024  # SDK limit, not a server claim.
    values = ('',) * MAX_HINTS_PER_CATEGORY
    config = ProtocolConfig(**{name: values})
    inner = input_boxes(initial_request(config))[0]
    assert parse_fields(inner) == [Field(3 if name == 'bias_terms' else 5, 2, b'')] * 1024
    with pytest.raises(ValueError, match='count limit'):
        ProtocolConfig(**{name: values + ('',)})


def test_count_budgets_are_per_category_not_shared():
    config = ProtocolConfig(bias_terms=('',) * 1024, preferences=('',) * 1024)
    assert len(parse_fields(input_boxes(initial_request(config))[0])) == 2048


def test_combined_hint_utf8_budget_accepts_exact_boundary():
    assert MAX_HINT_UTF8_BYTES == 1024 * 1024
    bias = 'é' * (MAX_HINT_UTF8_BYTES // 2 - 1)
    config = ProtocolConfig(bias_terms=(bias,), preferences=('é',))
    inner = parse_fields(input_boxes(initial_request(config))[0])
    assert inner == [Field(3, 2, bias.encode()), Field(5, 2, b'\xc3\xa9')]
    assert sum(len(item.value) for item in inner) == MAX_HINT_UTF8_BYTES


@pytest.mark.parametrize('name', ['bias_terms', 'preferences'])
def test_single_category_utf8_overflow(name):
    with pytest.raises(ValueError, match='UTF-8 byte limit'):
        ProtocolConfig(**{name: ('𝄞' * (MAX_HINT_UTF8_BYTES // 4) + 'x',)})


def test_hint_byte_budget_is_combined_and_counts_bytes_not_code_points():
    with pytest.raises(ValueError, match='preferences.*UTF-8 byte limit'):
        ProtocolConfig(bias_terms=('x' * (MAX_HINT_UTF8_BYTES - 3),), preferences=('éé',))


@pytest.mark.parametrize('name,limit', [
    ('bias_terms', MAX_HINT_UTF8_BYTES),
    ('preferences', MAX_HINT_UTF8_BYTES),
    ('secondary_languages', MAX_SECONDARY_LANGUAGE_UTF8_BYTES),
])
def test_oversized_text_is_rejected_before_encoding_or_wire_assembly(name, limit, monkeypatch):
    class NoEncoding(str):
        def encode(self, *args, **kwargs):
            pytest.fail('oversized text must be rejected before UTF-8 allocation')
    def no_wire(*args, **kwargs):
        pytest.fail('invalid config must never reach wire assembly')
    monkeypatch.setattr('rambler.protocol.string_field', no_wire)
    with pytest.raises(ValueError, match='UTF-8 byte limit'):
        ProtocolConfig(**{name: (NoEncoding('x' * (limit + 1)),)})


@pytest.mark.parametrize('primary', ['fr-FR', 'zh-CN', 'zh-Hans'])
def test_secondary_languages_exact_messages_preserve_primary_order_duplicates_and_raw_text(primary):
    languages = ('en-US', 'zh-CN', 'zh-Hans', 'en-US', ' raw ', 'é')
    config = ProtocolConfig(language=primary, secondary_languages=languages)
    expected_user = blob(2, blob(1, primary)) + b''.join(
        blob(4, blob(1, value) + b'\x10\x01') for value in languages
    )
    baseline = parse_fields(initial_request(ProtocolConfig(language=primary)))
    assert parse_fields(initial_request(config)) == [
        Field(293000, 2, expected_user) if item.number == 293000 else item for item in baseline
    ]
    user = parse_fields(expected_user)
    assert user[0] == Field(2, 2, blob(1, primary))
    for item, language in zip(user[1:], languages, strict=True):
        assert item.number == 4 and item.wire_type == 2
        assert parse_fields(item.value) == [Field(1, 2, language.encode()), Field(2, 0, 1)]
    assert input_boxes(initial_request(config)) == []


def test_secondary_language_sdk_limits_and_nonempty_requirement():
    assert MAX_SECONDARY_LANGUAGES == 16
    config = ProtocolConfig(secondary_languages=('en-US',) * MAX_SECONDARY_LANGUAGES)
    user = next(item.value for item in parse_fields(initial_request(config)) if item.number == 293000)
    assert len([item for item in parse_fields(user) if item.number == 4]) == 16
    with pytest.raises(ValueError, match='count limit'):
        ProtocolConfig(secondary_languages=('en-US',) * 17)
    with pytest.raises(ValueError, match='nonempty'):
        ProtocolConfig(secondary_languages=('',))
    assert MAX_SECONDARY_LANGUAGE_UTF8_BYTES == 1024
    exact = 'é' * (MAX_SECONDARY_LANGUAGE_UTF8_BYTES // 2)
    assert ProtocolConfig(secondary_languages=(exact,)).secondary_languages == (exact,)
    with pytest.raises(ValueError, match='UTF-8 byte limit'):
        ProtocolConfig(secondary_languages=(exact, 'x'))


@pytest.mark.parametrize('context,hex_vector', [
    (EditingContext('abc'), '9a4b050a03616263'),
    (EditingContext('a', 'b', 'c', True), '9a4b0b22090a01611201621a0163'),
    (EditingContext(), '9a4b020a00'),
    (EditingContext(cursor_aware=True), '9a4b0822060a0012001a00'),
])
def test_late_context_vectors_remain_strictly_unchanged(context, hex_vector):
    config = ProtocolConfig(bias_terms=('do not copy',), preferences=('nor this',))
    before = context_request(context)
    assert before == bytes.fromhex(hex_vector)
    initial_request(config)
    assert context_request(context) == before
    assert all(item.number in (1, 4) for item in parse_fields(input_boxes(before)[0]))


async def test_fake_engine_concurrent_and_reused_sessions_keep_hints_initial_only():
    class FakeTransport:
        def __init__(self):
            self.calls = []

        async def stream(self, requests, *, metadata, timeout):
            assert metadata == () and timeout > 0
            captured = []
            self.calls.append(captured)
            async for payload in requests:
                captured.append(payload)
                await asyncio.sleep(0)
            yield b'\x08\x01'  # Locally synthesized S3 END, no network or channel.

    transport = FakeTransport()
    engine = RamblerEngine(transport)
    config = ProtocolConfig(bias_terms=('contact', 'contact'), preferences=('raw preference',),
                            secondary_languages=('en-US',))

    async def run(index, session_config, cursor_aware):
        exhausted = False
        calls = 0

        async def audio():
            nonlocal exhausted
            yield str(index).encode()
            exhausted = True

        async def late_context():
            nonlocal calls
            assert exhausted
            calls += 1
            return EditingContext('a', 'b', 'c', True) if cursor_aware else EditingContext('abc')

        events = [event async for event in engine.stream(audio(), config=session_config, context=late_context)]
        assert calls == 1 and events[-1].response is None
        return events[-1].metrics.session_id

    ids = await asyncio.gather(
        run(0, config, False), run(1, ProtocolConfig(), True), run(2, config, True),
    )
    ids.append(await run(3, config, True))
    assert len(transport.calls) == len(set(ids)) == 4 and not engine._sessions
    for captured in transport.calls:
        assert len(captured) == 4
        index = int(next(item.value for item in parse_fields(
            next(item.value for item in parse_fields(captured[1]) if item.number == 293101)
        ) if item.number == 1))
        initial_boxes = input_boxes(captured[0])
        assert initial_boxes == ([] if index == 1 else [
            bytes.fromhex('1a07636f6e746163741a07636f6e746163742a0e72617720707265666572656e6365')
        ])
        assert input_boxes(captured[1]) == []
        assert captured[2] == bytes.fromhex(
            '9a4b050a03616263' if index == 0 else '9a4b0b22090a01611201621a0163'
        )
        assert captured[3] == b'\x10\x01\x18\x01'
        assert all(item.number in (1, 4) for item in parse_fields(input_boxes(captured[2])[0]))
    assert config.bias_terms == ('contact', 'contact')
    assert ProtocolConfig().bias_terms == ProtocolConfig().preferences == ()
    await engine.close()
