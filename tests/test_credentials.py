import pytest
from rambler.credentials import EnvironmentCredentials, validate_metadata

NAMES = ('RAMBLER_API_KEY', 'RAMBLER_ANDROID_PACKAGE', 'RAMBLER_ANDROID_CERT')


async def test_credentials_are_per_call_and_secret_not_repr(monkeypatch):
    for name in NAMES:
        monkeypatch.delenv(name, raising=False)
    provider = EnvironmentCredentials()
    assert await provider.metadata() == ()
    monkeypatch.setenv(NAMES[0], 'secret-123')
    with pytest.raises(ValueError) as error:
        await provider.metadata()
    assert 'secret-123' not in str(error.value) + repr(provider)
    monkeypatch.setenv(NAMES[1], 'package')
    monkeypatch.setenv(NAMES[2], 'certificate')
    assert await provider.metadata() == (('x-goog-api-key', 'secret-123'), ('x-android-package', 'package'), ('x-android-cert', 'certificate'))
    monkeypatch.setenv(NAMES[0], 'rotated')
    assert (await provider.metadata())[0] == ('x-goog-api-key', 'rotated')
    assert 'secret-123' not in repr(provider) and vars(provider) == {}


@pytest.mark.parametrize('metadata', [
    None, 'private-secret', b'private-secret', [('key',)],
    [('Upper', 'private-secret')], [('bad key', 'private-secret')],
    [('key', 'private-secret\r\n')], [('key', 'private-secret\x00')],
    [('key', 'private-secreté')], [('key', b'private-secret')],
    [('key-bin', 'private-secret')], [(1, 'private-secret')],
])
def test_metadata_validation_is_strict_and_redacted(metadata):
    with pytest.raises(ValueError) as error:
        validate_metadata(metadata)
    assert 'private-secret' not in str(error.value) + repr(error.value)


def test_metadata_snapshot_supports_binary_and_bounds_size():
    mutable = [['authorization', 'Bearer fake-value'], ['blob-bin', b'\x00\xff']]
    snapshot = validate_metadata(mutable)
    mutable[0][1] = 'rotated'
    assert snapshot == (('authorization', 'Bearer fake-value'), ('blob-bin', b'\x00\xff'))
    for metadata in ([('key', 'x')] * 101, [('key', 'x' * 65536)]):
        with pytest.raises(ValueError):
            validate_metadata(metadata)


@pytest.mark.parametrize('invalid', ['private-secret\n', 'private-secreté', 'private-secret\t'])
async def test_environment_rejects_values_before_native_grpc_logging(monkeypatch, capsys, invalid):
    for name, value in zip(NAMES, (invalid, 'package', 'certificate')):
        monkeypatch.setenv(name, value)
    with pytest.raises(ValueError) as error:
        await EnvironmentCredentials().metadata()
    assert 'private-secret' not in str(error.value)
    captured = capsys.readouterr()
    assert captured.out == captured.err == ''
