#!/usr/bin/env python3
"""Run a loopback-only development WSS operator with temporary, scoped TLS.

The public CA path is supplied only to child clients. No OS trust store is
changed, and no user identity, invitation or message is written by this helper.
"""
from __future__ import annotations

import argparse
import ctypes
from ctypes import wintypes
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time


class OperatorError(RuntimeError):
    """Only fixed phase names belong in this error, never operational data."""


class ProcessGroup:
    """Own only processes started here; Windows jobs contain their descendants."""

    def __init__(self):
        self.processes: list[subprocess.Popen] = []
        self.job = None
        if os.name != 'nt':
            return

        class BasicLimits(ctypes.Structure):
            _fields_ = [
                ('PerProcessUserTimeLimit', ctypes.c_longlong),
                ('PerJobUserTimeLimit', ctypes.c_longlong),
                ('LimitFlags', wintypes.DWORD),
                ('MinimumWorkingSetSize', ctypes.c_size_t),
                ('MaximumWorkingSetSize', ctypes.c_size_t),
                ('ActiveProcessLimit', wintypes.DWORD),
                ('Affinity', ctypes.c_size_t),
                ('PriorityClass', wintypes.DWORD),
                ('SchedulingClass', wintypes.DWORD),
            ]

        class IoCounters(ctypes.Structure):
            _fields_ = [(name, ctypes.c_ulonglong) for name in (
                'ReadOperationCount', 'WriteOperationCount', 'OtherOperationCount',
                'ReadTransferCount', 'WriteTransferCount', 'OtherTransferCount')]

        class ExtendedLimits(ctypes.Structure):
            _fields_ = [
                ('BasicLimitInformation', BasicLimits), ('IoInfo', IoCounters),
                ('ProcessMemoryLimit', ctypes.c_size_t),
                ('JobMemoryLimit', ctypes.c_size_t),
                ('PeakProcessMemoryUsed', ctypes.c_size_t),
                ('PeakJobMemoryUsed', ctypes.c_size_t),
            ]

        kernel = ctypes.WinDLL('kernel32', use_last_error=True)
        kernel.CreateJobObjectW.argtypes = [ctypes.c_void_p, wintypes.LPCWSTR]
        kernel.CreateJobObjectW.restype = wintypes.HANDLE
        kernel.SetInformationJobObject.argtypes = [
            wintypes.HANDLE, ctypes.c_int, ctypes.c_void_p, wintypes.DWORD]
        kernel.SetInformationJobObject.restype = wintypes.BOOL
        kernel.OpenProcess.argtypes = [wintypes.DWORD, wintypes.BOOL, wintypes.DWORD]
        kernel.OpenProcess.restype = wintypes.HANDLE
        kernel.AssignProcessToJobObject.argtypes = [wintypes.HANDLE, wintypes.HANDLE]
        kernel.AssignProcessToJobObject.restype = wintypes.BOOL
        kernel.CloseHandle.argtypes = [wintypes.HANDLE]
        kernel.CloseHandle.restype = wintypes.BOOL
        self.kernel = kernel
        job = kernel.CreateJobObjectW(None, None)
        if not job:
            raise OperatorError('job_creation_failed')
        limits = ExtendedLimits()
        limits.BasicLimitInformation.LimitFlags = 0x2000  # KILL_ON_JOB_CLOSE
        if not kernel.SetInformationJobObject(job, 9, ctypes.byref(limits), ctypes.sizeof(limits)):
            kernel.CloseHandle(job)
            raise OperatorError('job_configuration_failed')
        self.job = job

    def spawn(self, command, **options):
        if os.name == 'nt':
            options.setdefault('creationflags', subprocess.CREATE_NO_WINDOW)
        else:
            options.setdefault('start_new_session', True)
        process = subprocess.Popen(command, **options)
        self.processes.append(process)
        if self.job:
            # The test coordinator and server are owned by this invocation.
            # Assignment happens before any request is sent to either process.
            handle = self.kernel.OpenProcess(0x0100 | 0x0001, False, process.pid)
            try:
                if not handle or not self.kernel.AssignProcessToJobObject(self.job, handle):
                    process.kill()
                    process.wait()
                    raise OperatorError('job_assignment_failed')
            finally:
                if handle:
                    self.kernel.CloseHandle(handle)
        return process

    def close(self):
        closed_job = bool(self.job)
        if self.job:
            self.kernel.CloseHandle(self.job)
            self.job = None
        for process in self.processes:
            if not closed_job and process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=5)
            for stream in (process.stdin, process.stdout, process.stderr):
                if stream:
                    stream.close()
        self.processes.clear()

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()


def find_openssl() -> str:
    executable = shutil.which('openssl')
    if executable:
        return executable
    for base in (os.environ.get('ProgramFiles', r'C:\Program Files'),
                 os.environ.get('ProgramFiles(x86)', r'C:\Program Files (x86)')):
        for relative in ('Git/ucrt64/bin/openssl.exe', 'Git/usr/bin/openssl.exe'):
            candidate = Path(base) / relative
            if candidate.is_file():
                return str(candidate)
    raise OperatorError('openssl_not_found')


class LocalOperator:
    def __init__(self, binary: Path, port: int = 0, environment=None):
        self.binary = binary.resolve()
        if not self.binary.is_file() or not 0 <= port <= 65535:
            raise OperatorError('invalid_operator_configuration')
        self.port = port
        self.environment = dict(os.environ if environment is None else environment)
        self.directory = None
        self.group = None
        self.process = None

    def __enter__(self):
        try:
            openssl = find_openssl()
            self.temp_parent = Path(tempfile.gettempdir()).resolve()
            self.temporary = tempfile.TemporaryDirectory(prefix='veyra-local-operator-', dir=self.temp_parent)
            self.directory = Path(self.temporary.name).resolve()
            self._verify_owned_directory()
            directory = self.directory

            def quiet(arguments):
                try:
                    subprocess.run([openssl, *arguments], check=True, timeout=30,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                   creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
                except (OSError, subprocess.SubprocessError) as error:
                    raise OperatorError('certificate_generation_failed') from error

            # These keys are synthetic operator TLS configuration, never user keys.
            quiet(['req', '-x509', '-newkey', 'rsa:2048', '-nodes',
                   '-keyout', str(directory/'ca.key'), '-out', str(directory/'ca.pem'),
                   '-days', '1', '-subj', '/CN=Veyra local development CA',
                   '-addext', 'basicConstraints=critical,CA:TRUE',
                   '-addext', 'keyUsage=critical,keyCertSign,cRLSign'])
            quiet(['req', '-new', '-newkey', 'rsa:2048', '-nodes',
                   '-keyout', str(directory/'server.key'), '-out', str(directory/'server.csr'),
                   '-subj', '/CN=localhost'])
            (directory/'server.ext').write_text(
                'basicConstraints=critical,CA:FALSE\n'
                'keyUsage=critical,digitalSignature,keyEncipherment\n'
                'extendedKeyUsage=serverAuth\n'
                'subjectAltName=DNS:localhost,IP:127.0.0.1\n', encoding='utf-8')
            quiet(['x509', '-req', '-in', str(directory/'server.csr'),
                   '-CA', str(directory/'ca.pem'), '-CAkey', str(directory/'ca.key'),
                   '-CAcreateserial', '-out', str(directory/'server.pem'),
                   '-days', '1', '-extfile', str(directory/'server.ext')])

            if self.port == 0:
                with socket.socket() as listener:
                    listener.bind(('127.0.0.1', 0))
                    self.port = listener.getsockname()[1]
            self.group = ProcessGroup()
            self.process = self.group.spawn(
                [str(self.binary), f'127.0.0.1:{self.port}',
                 str(directory/'server.pem'), str(directory/'server.key')],
                env=self.environment, stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            self.endpoint = f'wss://127.0.0.1:{self.port}/v1/rendezvous'
            # Keep existing trusted roots available to Cargo and other development
            # children, instead of replacing their CA set with only the local CA.
            context = ssl.create_default_context()
            previous_ca_file = self.environment.get('SSL_CERT_FILE')
            previous_ca_directory = self.environment.get('SSL_CERT_DIR')
            if previous_ca_file or previous_ca_directory:
                context.load_verify_locations(cafile=previous_ca_file or None,
                                              capath=previous_ca_directory or None)
            roots = ''.join(ssl.DER_cert_to_PEM_cert(value)
                            for value in context.get_ca_certs(binary_form=True))
            self.ca_file = directory/'ca-bundle.pem'
            self.ca_file.write_text(roots + (directory/'ca.pem').read_text(encoding='ascii'),
                                    encoding='ascii')
            context.load_verify_locations(cafile=str(directory/'ca.pem'))
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                if self.process.poll() is not None:
                    raise OperatorError('operator_exited_before_ready')
                try:
                    with socket.create_connection(('127.0.0.1', self.port), timeout=.25) as connection:
                        with context.wrap_socket(connection, server_hostname='127.0.0.1'):
                            return self
                except (OSError, ssl.SSLError):
                    time.sleep(.1)
            raise OperatorError('operator_tls_readiness_timeout')
        except BaseException:
            self.close()
            raise

    def public_configuration(self):
        return {'endpoint': self.endpoint, 'caFile': str(self.ca_file), 'scope': 'local-development'}

    def _verify_owned_directory(self):
        # Verify the exact recursive cleanup target before TemporaryDirectory removes it.
        resolved = self.directory.resolve()
        if resolved.parent != self.temp_parent or not resolved.name.startswith('veyra-local-operator-'):
            raise OperatorError('unexpected_temporary_directory')

    def close(self):
        try:
            if self.group:
                self.group.close()
                self.group = None
        finally:
            if self.directory:
                self._verify_owned_directory()
                self.temporary.cleanup()
                self.directory = None

    def __exit__(self, *_):
        self.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--serve', action='store_true', required=True)
    parser.add_argument('--port', type=int, default=0)
    arguments = parser.parse_args()
    stopping = threading.Event()

    def stop(*_):
        stopping.set()

    signal.signal(signal.SIGINT, stop)
    signal.signal(signal.SIGTERM, stop)
    try:
        with LocalOperator(arguments.binary, arguments.port) as operator:
            # This is the entire stdout protocol: one public configuration line.
            print(json.dumps(operator.public_configuration(), ensure_ascii=True), flush=True)

            def read_control():
                try:
                    for line in sys.stdin:
                        if line.strip() == 'stop':
                            break
                finally:
                    stopping.set()

            threading.Thread(target=read_control, daemon=True).start()
            while not stopping.wait(.2):
                if operator.process.poll() is not None:
                    raise OperatorError('operator_exited')
        return 0
    except OperatorError as error:
        print(f'Local development operator failed: {error}', file=sys.stderr)
        return 1
    except (OSError, subprocess.SubprocessError):
        print('Local development operator failed: process_or_file_error', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
