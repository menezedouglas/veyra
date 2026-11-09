#!/usr/bin/env python3
"""Exercise the real Windows product IPC, WSS, Noise and one-time messages.

Uses only synthetic data in the Rust product-session test. TLS keys are
temporary operator configuration. Reports contain outcomes and executable
hashes, never tokens, fingerprints, keys, SDP, addresses or message contents.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('veyra_local_operator', ROOT/'scripts/local-operator.py')
LOCAL = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(LOCAL)


def build_environment():
    environment = os.environ.copy()
    cargo_bin = Path(environment.get('CARGO_HOME', Path.home()/'.cargo'))/'bin'
    environment['PATH'] = str(cargo_bin) + os.pathsep + environment.get('PATH', '')
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT/'rust-toolchain.toml').read_text(encoding='utf-8'), re.MULTILINE)
    if not channel:
        raise LOCAL.OperatorError('missing_pinned_toolchain')
    machine = 'aarch64' if platform.machine().lower() in ('arm64', 'aarch64') else 'x86_64'
    environment['RUSTUP_TOOLCHAIN'] = f'{channel[1]}-{machine}-pc-windows-msvc'
    environment['CARGO_TERM_COLOR'] = 'never'
    environment['CARGO_TERM_PROGRESS_WHEN'] = 'never'
    return environment


def compile_artifacts(environment, release):
    cargo = shutil.which('cargo', path=environment['PATH'])
    if not cargo:
        raise LOCAL.OperatorError('cargo_not_found')
    profile = ['--release'] if release else []

    def compile_command(arguments):
        result = subprocess.run([cargo, *arguments, *profile, '--message-format=json'],
                                cwd=ROOT, env=environment, stdout=subprocess.PIPE,
                                text=True, encoding='utf-8')
        if result.returncode:
            raise LOCAL.OperatorError('product_build_failed')
        entries = []
        for line in result.stdout.splitlines():
            entry = json.loads(line)
            if entry.get('reason') == 'compiler-artifact' and entry.get('executable'):
                entries.append(entry)
        return entries

    artifacts = compile_command(['build', '--locked', '-p', 'veyra-daemon', '-p', 'veyra-signaling'])
    tests = compile_command(['test', '--locked', '-p', 'veyra-daemon', '--test', 'session_process', '--no-run'])

    def named(entries, name):
        found = {Path(entry['executable']).resolve() for entry in entries
                 if entry.get('target', {}).get('name') == name}
        if len(found) != 1:
            raise LOCAL.OperatorError('ambiguous_compiled_artifact')
        return found.pop()

    return {
        'signaling': named(artifacts, 'veyra-signaling'),
        'daemon': named(artifacts, 'veyra-daemon'),
        'vault': named(artifacts, 'veyra-vault'),
        'test': named(tests, 'session_process'),
    }


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--no-build', action='store_true', help='Use explicit existing executables.')
    parser.add_argument('--test-binary', type=Path)
    parser.add_argument('--signaling', type=Path)
    parser.add_argument('--daemon', type=Path)
    parser.add_argument('--vault', type=Path)
    parser.add_argument('--release', action='store_true')
    parser.add_argument('--port', type=int, default=0)
    parser.add_argument('--timeout', type=int, default=180)
    parser.add_argument('--report', type=Path)
    arguments = parser.parse_args()
    if os.name != 'nt':
        parser.error('Run this real-process laboratory on Windows.')
    if not 0 <= arguments.port <= 65535 or not 1 <= arguments.timeout <= 600:
        parser.error('Invalid port or timeout.')
    if arguments.no_build and not arguments.test_binary:
        parser.error('--no-build requires --test-binary from cargo test --no-run output.')

    report = {
        'platform': 'windows',
        'executedAtUtc': datetime.now(timezone.utc).isoformat(),
        'checks': {},
        'failure': None,
    }
    checks = report['checks']
    try:
        environment = build_environment()
        report['execution'] = {
            'profile': 'release' if arguments.release else 'debug',
            'toolchain': environment['RUSTUP_TOOLCHAIN'],
            'scope': 'same-host-core-product-processes',
            'artifacts': 'provided' if arguments.no_build else 'cargo-build',
            'graphicalValidation': False,
            'installationValidation': False,
        }
        if arguments.no_build:
            directory = ROOT/'target'/('release' if arguments.release else 'debug')
            binaries = {
                'signaling': directory/'veyra-signaling.exe',
                'daemon': directory/'veyra-daemon.exe',
                'vault': directory/'veyra-vault.exe',
                'test': arguments.test_binary,
            }
        else:
            binaries = compile_artifacts(environment, arguments.release)
        for role, value in [('signaling', arguments.signaling), ('daemon', arguments.daemon),
                            ('vault', arguments.vault), ('test', arguments.test_binary)]:
            if value is not None:
                binaries[role] = value
        binaries = {role: path.resolve() for role, path in binaries.items()}
        if not all(path.is_file() for path in binaries.values()):
            raise LOCAL.OperatorError('missing_executable')
        initial_hashes = {role: digest(path) for role, path in binaries.items()}
        report['binaries'] = {role: {'sha256': value} for role, value in initial_hashes.items()}

        with LOCAL.LocalOperator(binaries['signaling'], arguments.port, environment) as operator:
            checks['real_wss_with_scoped_trusted_ca'] = True
            client_environment = environment.copy()
            client_environment.update({
                'SSL_CERT_FILE': str(operator.ca_file),
                'VEYRA_TEST_WSS_ENDPOINT': operator.endpoint,
                'VEYRA_TEST_RELAY': '0',
                'VEYRA_TEST_DAEMON_BIN': str(binaries['daemon']),
                'VEYRA_TEST_VAULT_BIN': str(binaries['vault']),
                'VEYRA_TEST_REQUIRE_EMPTY_PATH': '1',
                'PATH': '',
            })
            with LOCAL.ProcessGroup() as clients:
                process = clients.spawn(
                    [str(binaries['test']), 'product_session_bidirectional', '--exact',
                     '--ignored', '--nocapture', '--test-threads=1'],
                    cwd=ROOT, env=client_environment, stdin=subprocess.DEVNULL,
                    stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
                try:
                    output, _ = process.communicate(timeout=arguments.timeout)
                except subprocess.TimeoutExpired as error:
                    raise LOCAL.OperatorError('product_session_timeout') from error
                if process.returncode:
                    report['testExitCode'] = process.returncode
                    raise LOCAL.OperatorError('product_session_failed')
                # Both markers occur only after all Rust assertions completed.
                if (b'test result: ok. 1 passed;' not in output
                        or b'selected route Direct' not in output):
                    raise LOCAL.OperatorError('product_test_not_executed')
                del output
            checks.update({
                'explicit_client_paths_without_node_or_rust_on_path': True,
                'two_daemons_and_two_isolated_vaults': True,
                'validated_invitation_binding_and_noise_pairing': True,
                'matching_transcript_codes_and_explicit_approval': True,
                'ciphertext_only_network_path': True,
                'bidirectional_hpke_messages': True,
                'opening_requires_confirmation_and_is_consumed_once': True,
                'selected_ice_route_is_direct': True,
                'revocation_discards_pending_message': True,
            })
        checks['owned_processes_and_temporary_tls_cleaned'] = True
        if any(digest(path) != initial_hashes[role] for role, path in binaries.items()):
            raise LOCAL.OperatorError('executables_changed_during_test')
        checks['executables_unchanged_by_laboratory'] = True
    except LOCAL.OperatorError as error:
        report['failure'] = str(error)
    except (OSError, subprocess.SubprocessError, ValueError):
        report['failure'] = 'laboratory_process_or_file_error'
    serialized = json.dumps(report, ensure_ascii=True, indent=2) + '\n'
    if arguments.report:
        arguments.report.parent.mkdir(parents=True, exist_ok=True)
        arguments.report.write_text(serialized, encoding='utf-8')
    print(serialized, end='')
    return 1 if report['failure'] else 0


if __name__ == '__main__':
    raise SystemExit(main())
