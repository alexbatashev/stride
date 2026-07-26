import asyncio as _stride_asyncio
import base64 as _stride_base64
import os as _stride_os
import shlex as _stride_shlex
import subprocess as _stride_subprocess
import sys as _stride_sys

_STRIDE_EXEC_CALLBACK = "__stride_exec"

try:
    _stride_os.chdir("/home/agent")
except OSError:
    pass


def _stride_sync_await(coroutine):
    try:
        coroutine.send(None)
    except StopIteration as completed:
        return completed.value
    finally:
        coroutine.close()
    raise RuntimeError("blocking callback unexpectedly suspended")


def _stride_bytes(value, text_mode, encoding, errors):
    if value is None:
        return b""
    if isinstance(value, str):
        if not text_mode:
            raise TypeError("write() argument must be bytes, not str")
        return value.encode(encoding or "utf-8", errors or "strict")
    if isinstance(value, (bytes, bytearray, memoryview)):
        if text_mode:
            raise TypeError("write() argument must be str, not bytes")
        return bytes(value)
    raise TypeError("input must be str or bytes-like")


def _stride_decode(value, text_mode, encoding, errors):
    if not text_mode:
        return value
    return value.decode(encoding or "utf-8", errors or "strict")


def _stride_emit(value, stream):
    if not value:
        return
    buffer = getattr(stream, "buffer", None)
    if buffer is not None:
        buffer.write(value)
        buffer.flush()
    else:
        stream.write(value.decode("utf-8", "replace"))
        stream.flush()


def _stride_command(args, shell):
    if shell:
        if isinstance(args, (str, bytes, _stride_os.PathLike)):
            line = _stride_os.fsdecode(args)
        else:
            line = _stride_shlex.join([_stride_os.fsdecode(arg) for arg in args])
        return {"line": line}
    if isinstance(args, (str, bytes, _stride_os.PathLike)):
        argv = [_stride_os.fsdecode(args)]
    else:
        argv = [_stride_os.fsdecode(arg) for arg in args]
    if not argv:
        raise ValueError("args must not be empty")
    return {"argv": argv}


def _stride_run(
    args,
    *,
    stdin=None,
    input=None,
    stdout=None,
    stderr=None,
    capture_output=False,
    shell=False,
    cwd=None,
    timeout=None,
    check=False,
    text=False,
    encoding=None,
    errors=None,
    universal_newlines=None,
    env=None,
    preexec_fn=None,
    start_new_session=False,
    executable=None,
    **kwargs,
):
    if kwargs:
        name = next(iter(kwargs))
        raise TypeError("unsupported subprocess argument: %s" % name)
    if preexec_fn is not None or start_new_session:
        raise NotImplementedError("preexec_fn and process sessions are not supported")
    if executable is not None:
        raise NotImplementedError("executable overrides are not supported")
    if input is not None and stdin is not None:
        raise ValueError("stdin and input arguments may not both be used")
    if capture_output and (stdout is not None or stderr is not None):
        raise ValueError("stdout and stderr arguments may not be used with capture_output")
    text_mode = bool(text or universal_newlines or encoding or errors)
    data = _stride_bytes(input, text_mode, encoding, errors)
    if stdin not in (None, _stride_subprocess.PIPE, _stride_subprocess.DEVNULL):
        raise NotImplementedError("only stdin=PIPE or DEVNULL is supported")
    if stdin == _stride_subprocess.DEVNULL:
        data = b""
    payload = _stride_command(args, shell)
    payload.update(
        stdin_b64=_stride_base64.b64encode(data).decode("ascii"),
        cwd=_stride_os.fspath(cwd) if cwd is not None else _stride_os.getcwd(),
        timeout_ms=None if timeout is None else max(0, int(float(timeout) * 1000)),
    )
    response = _stride_sync_await(invoke(_STRIDE_EXEC_CALLBACK, **payload))
    if not isinstance(response, dict) or "error" in response:
        message = response.get("error") if isinstance(response, dict) else response
        raise RuntimeError("command bridge failed: %s" % message)
    out_bytes = _stride_base64.b64decode(response.get("stdout_b64", ""))
    err_bytes = _stride_base64.b64decode(response.get("stderr_b64", ""))
    if response.get("timed_out"):
        raise _stride_subprocess.TimeoutExpired(
            args,
            timeout,
            output=_stride_decode(out_bytes, text_mode, encoding, errors),
            stderr=_stride_decode(err_bytes, text_mode, encoding, errors),
        )
    capture_stdout = capture_output or stdout == _stride_subprocess.PIPE
    capture_stderr = capture_output or stderr in (
        _stride_subprocess.PIPE,
        _stride_subprocess.STDOUT,
    )
    if stderr == _stride_subprocess.STDOUT:
        out_bytes += err_bytes
        err_bytes = b""
        capture_stderr = False
    if stdout == _stride_subprocess.DEVNULL:
        out_bytes = b""
    elif not capture_stdout:
        _stride_emit(out_bytes, _stride_sys.stdout)
    if stderr == _stride_subprocess.DEVNULL:
        err_bytes = b""
    elif not capture_stderr:
        _stride_emit(err_bytes, _stride_sys.stderr)
    completed = _stride_subprocess.CompletedProcess(
        args,
        int(response.get("returncode", 1)),
        _stride_decode(out_bytes, text_mode, encoding, errors) if capture_stdout else None,
        _stride_decode(err_bytes, text_mode, encoding, errors)
        if capture_stderr and stderr != _stride_subprocess.STDOUT
        else None,
    )
    completed.truncated = bool(response.get("truncated"))
    if check:
        completed.check_returncode()
    return completed


def _stride_call(*popenargs, timeout=None, **kwargs):
    return _stride_run(*popenargs, timeout=timeout, **kwargs).returncode


def _stride_check_call(*popenargs, **kwargs):
    kwargs["check"] = True
    return _stride_run(*popenargs, **kwargs).returncode


def _stride_check_output(*popenargs, timeout=None, **kwargs):
    if "stdout" in kwargs:
        raise ValueError("stdout argument not allowed, it will be overridden")
    kwargs["stdout"] = _stride_subprocess.PIPE
    kwargs["check"] = True
    return _stride_run(*popenargs, timeout=timeout, **kwargs).stdout


def _stride_getstatusoutput(command, *, encoding=None, errors=None):
    result = _stride_run(
        command,
        shell=True,
        stdout=_stride_subprocess.PIPE,
        stderr=_stride_subprocess.STDOUT,
        text=True,
        encoding=encoding,
        errors=errors,
    )
    return result.returncode, result.stdout.rstrip("\n")


def _stride_getoutput(command, *, encoding=None, errors=None):
    return _stride_getstatusoutput(command, encoding=encoding, errors=errors)[1]


class _StrideUnsupportedStream:
    def read(self, *args, **kwargs):
        raise NotImplementedError("Popen streaming is not supported; use communicate()")

    readline = read
    readlines = read

    def __iter__(self):
        raise NotImplementedError("Popen streaming is not supported; use communicate()")


class _StridePopen:
    def __init__(
        self,
        args,
        bufsize=-1,
        executable=None,
        stdin=None,
        stdout=None,
        stderr=None,
        preexec_fn=None,
        close_fds=True,
        shell=False,
        cwd=None,
        env=None,
        universal_newlines=None,
        startupinfo=None,
        creationflags=0,
        restore_signals=True,
        start_new_session=False,
        pass_fds=(),
        *,
        user=None,
        group=None,
        extra_groups=None,
        encoding=None,
        errors=None,
        text=None,
        umask=-1,
        pipesize=-1,
        process_group=None,
    ):
        unsupported = (
            executable is not None
            or preexec_fn is not None
            or startupinfo is not None
            or creationflags
            or start_new_session
            or pass_fds
            or user is not None
            or group is not None
            or extra_groups is not None
            or umask != -1
            or process_group is not None
        )
        if unsupported:
            raise NotImplementedError("requested Popen process controls are not supported")
        if bufsize not in (-1, 0) or pipesize not in (-1, 0):
            raise NotImplementedError("Popen buffering controls are not supported")
        self.args = args
        self.returncode = None
        self.pid = None
        self._kwargs = dict(
            stdin=stdin,
            stdout=stdout,
            stderr=stderr,
            shell=shell,
            cwd=cwd,
            env=env,
            universal_newlines=universal_newlines,
            encoding=encoding,
            errors=errors,
            text=bool(text),
        )
        self._completed = None
        self.stdin = None
        self.stdout = _StrideUnsupportedStream() if stdout == _stride_subprocess.PIPE else None
        self.stderr = _StrideUnsupportedStream() if stderr == _stride_subprocess.PIPE else None

    def _finish(self, input=None, timeout=None):
        if self._completed is None:
            self._completed = _stride_run(
                self.args, input=input, timeout=timeout, **self._kwargs
            )
            self.returncode = self._completed.returncode
            self.stdout = self._completed.stdout
            self.stderr = self._completed.stderr
        elif input is not None:
            raise ValueError("cannot send input after communication started")
        return self._completed

    def communicate(self, input=None, timeout=None):
        completed = self._finish(input=input, timeout=timeout)
        return completed.stdout, completed.stderr

    def wait(self, timeout=None):
        return self._finish(timeout=timeout).returncode

    def poll(self):
        if self.returncode is None:
            raise NotImplementedError("Popen does not run in the background; call wait()")
        return self.returncode

    def send_signal(self, signal):
        raise NotImplementedError("Popen signals are not supported")

    def terminate(self):
        raise NotImplementedError("Popen signals are not supported")

    def kill(self):
        raise NotImplementedError("Popen signals are not supported")

    def __enter__(self):
        return self

    def __exit__(self, exc_type, value, traceback):
        if self.returncode is None:
            self.wait()


def _stride_system(line):
    returncode = _stride_run(line, shell=True).returncode
    return returncode << 8 if returncode >= 0 else returncode


_stride_subprocess.run = _stride_run
_stride_subprocess.call = _stride_call
_stride_subprocess.check_call = _stride_check_call
_stride_subprocess.check_output = _stride_check_output
_stride_subprocess.getstatusoutput = _stride_getstatusoutput
_stride_subprocess.getoutput = _stride_getoutput
_stride_subprocess.Popen = _StridePopen
_stride_os.system = _stride_system
