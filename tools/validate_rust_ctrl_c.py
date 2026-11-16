"""Windows-only development proof: isolated console Ctrl-C, loopback RPC only."""
import asyncio
import ctypes
import json
from pathlib import Path
import subprocess
import sys
import grpc

async def main(root, output):
    if sys.platform != 'win32':
        raise RuntimeError('Windows console test only')
    ROOT, OUTPUT = root.resolve(), output.resolve()
    if OUTPUT.exists():
        raise ValueError('output must be new')
    if not (ROOT / 'target/release/rambler.exe').is_file() or not (ROOT / 'artifacts/synthetic-v1/plain-en-16k.wav').is_file():
        raise ValueError('build the native audio CLI and prepare the controlled fixture first')
    received = asyncio.Event()
    cancelled = asyncio.Event()
    count = 0

    async def rpc(requests, context):
        nonlocal count
        try:
            async for _ in requests:
                count += 1
                if count == 1:
                    yield b'\x08\x00'
                if count >= 2:
                    received.set()
            await asyncio.Future()
        finally:
            cancelled.set()

    server = grpc.aio.server()
    server.add_generic_rpc_handlers((grpc.method_handlers_generic_handler(
        'speech.s3.S3StubbyClientConnectorService',
        {'S3Bidi':grpc.stream_stream_rpc_method_handler(rpc,
          request_deserializer=lambda value:value, response_serializer=lambda value:value)}),))
    port = server.add_insecure_port('127.0.0.1:0')
    await server.start()
    startup = subprocess.STARTUPINFO()
    startup.dwFlags |= subprocess.STARTF_USESHOWWINDOW
    startup.wShowWindow = subprocess.SW_HIDE
    process = None
    kernel = ctypes.WinDLL('kernel32', use_last_error=True)
    kernel.AttachConsole.argtypes = [ctypes.c_uint32]
    kernel.AttachConsole.restype = ctypes.c_int
    kernel.FreeConsole.restype = ctypes.c_int
    kernel.SetConsoleCtrlHandler.argtypes = [ctypes.c_void_p,ctypes.c_int]
    kernel.SetConsoleCtrlHandler.restype = ctypes.c_int
    kernel.GetConsoleProcessList.argtypes = [ctypes.POINTER(ctypes.c_uint32),ctypes.c_uint32]
    kernel.GetConsoleProcessList.restype = ctypes.c_uint32
    kernel.GenerateConsoleCtrlEvent.argtypes = [ctypes.c_uint32,ctypes.c_uint32]
    kernel.GenerateConsoleCtrlEvent.restype = ctypes.c_int
    attached = False
    try:
        # Do not pass an inherited ignore-Ctrl-C attribute to the new child.
        if not kernel.SetConsoleCtrlHandler(None,0):
            raise OSError(ctypes.get_last_error(),'reset inherited ignore flag failed')
        process = await asyncio.create_subprocess_exec(
            str(ROOT/'target/release/rambler.exe'), 'dictate-wav',
            str(ROOT/'artifacts/synthetic-v1/plain-en-16k.wav'), '--realtime',
            '--language','en-US','--timeout','30','--loopback',f'127.0.0.1:{port}',
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
            creationflags=subprocess.CREATE_NEW_CONSOLE, startupinfo=startup)
        await asyncio.wait_for(received.wait(),5)
        # Detach only this development process, never the parent console.
        kernel.FreeConsole()
        if not kernel.AttachConsole(process.pid):
            raise OSError(ctypes.get_last_error(),'AttachConsole failed')
        attached = True
        if not kernel.SetConsoleCtrlHandler(None,1):
            raise OSError(ctypes.get_last_error(),'ignore handler failed')
        pids = (ctypes.c_uint32*16)()
        size = kernel.GetConsoleProcessList(pids,16)
        own_pid = __import__('os').getpid()
        # Refuse broadcast unless this is precisely our new isolated child console.
        assert 1 <= size <= 16
        console_pids = set(pids[:size])
        assert console_pids == {own_pid,process.pid}, console_pids
        if not kernel.GenerateConsoleCtrlEvent(0,0):
            raise OSError(ctypes.get_last_error(),'GenerateConsoleCtrlEvent failed')
        # Do not keep the child's console host and its pipe handles alive.
        kernel.FreeConsole()
        attached = False
        try:
            await asyncio.wait_for(process.wait(),5)
            stdout,stderr = await asyncio.wait_for(process.communicate(),5)
        except TimeoutError:
            with OUTPUT.with_suffix('.diagnostic.json').open('x',encoding='utf-8') as f:
                json.dump({'child_exit_code':process.returncode,'rpc_cancelled':cancelled.is_set(),
                           'rpc_requests_received':count},f)
            raise
        await asyncio.wait_for(cancelled.wait(),5)
        value = json.loads(stdout.decode('utf-8'))
        success = (process.returncode==130 and value.get('type')=='error'
                   and value.get('category')=='cancelled' and not stderr)
        result = {'kind':'native_rust_windows_console_ctrl_c_loopback',
                  'successful':success,'exit_code':process.returncode,
                  'rpc_requests_received':count,'server_rpc_cancelled':cancelled.is_set(),
                  'isolated_new_console_verified':True,'stdout':value,'stderr_bytes':len(stderr),
                  'google_contacted':False}
        with OUTPUT.open('x',encoding='utf-8') as f:
            json.dump(result,f,ensure_ascii=False,indent=2)
            f.write('\n')
        print(json.dumps(result),flush=True)
        return 0 if success else 1
    finally:
        if process is not None and process.returncode is None:
            process.kill()
            await process.wait()
        if attached:
            kernel.FreeConsole()
        await server.stop(0)

if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('repo', type=Path)
    parser.add_argument('output', type=Path)
    options = parser.parse_args()
    raise SystemExit(asyncio.run(main(options.repo, options.output)))
