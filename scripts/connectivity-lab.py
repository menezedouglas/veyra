#!/usr/bin/env python3
"""Isolated, synthetic Coturn proof. No changes to host firewall or networking.

With --native, the caller MUST first build veyra-tests with the
veyra-transport/native-libnice feature and the checked-in CMake toolchain.
TCP/TLS is never reported as passed by the UDP-only Rust backend.
"""
import argparse
import json
import pathlib
import subprocess
import sys
import os

ROOT = pathlib.Path(__file__).resolve().parents[1]
IMAGE = "coturn/coturn:4.18.0@sha256:bbefd3e1fdfdc0d58770fe01b581fd8b00d9f3a5580d00acb77cf719a6bc78e3"


def command(args, **kwargs):
    return subprocess.run(args, check=True, cwd=ROOT, **kwargs)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--native", action="store_true")
    parser.add_argument("--docker-container", help="Existing isolated container containing the peer binaries")
    parser.add_argument("--binary", default="target/debug/veyra-tests")
    parser.add_argument("--protocol", choices=["udp", "tcp", "tls"], help="Run only one relay protocol")
    args = parser.parse_args()
    name = "veyra-connectivity-proof-" + str(os.getpid())
    network = name
    lab = ROOT / ".veyra-labs" / str(os.getpid())
    lab.mkdir(parents=True, exist_ok=True)
    def certificate(address="127.0.0.1"):
        # Synthetic operator TLS key; never an identity/message key.
        command(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(lab / "key.pem"), "-out", str(lab / "cert.pem"), "-subj", "/CN=" + name, "-addext", "subjectAltName=DNS:" + name + ",IP:" + address, "-addext", "basicConstraints=critical,CA:FALSE", "-addext", "extendedKeyUsage=serverAuth", "-days", "1"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        (lab / "key.pem").chmod(0o644)
        return command(["openssl", "x509", "-in", str(lab / "cert.pem"), "-outform", "DER"], capture_output=True).stdout
    root_der = certificate()
    config = (ROOT / "infrastructure/coturn/laboratory.conf").read_text().replace("no-tls\n", "tls-listening-port=5349\ncert=/etc/veyra/cert.pem\npkey=/etc/veyra/key.pem\n")
    (lab / "turnserver.conf").write_text(config)
    command(["docker", "network", "create", network], stdout=subprocess.DEVNULL)
    connected = False
    try:
        if args.docker_container:
            command(["docker", "network", "connect", network, args.docker_container])
            connected = True
        command(["docker", "run", "-d", "--name", name, "--network", network, "--network-alias", name, "--log-driver", "none", "--read-only", "--cap-drop", "ALL", "--cap-add", "NET_BIND_SERVICE", "--security-opt", "no-new-privileges", "--memory", "128m", "--pids-limit", "64", "--tmpfs", "/run:rw,noexec,nosuid,size=4m", "--tmpfs", "/tmp:rw,noexec,nosuid,size=4m", "--tmpfs", "/var/lib/coturn:rw,noexec,nosuid,size=4m", "--mount", "type=bind,src=" + str(lab) + ",dst=/etc/veyra,readonly", "--entrypoint", "turnserver", IMAGE, "-c", "/etc/veyra/turnserver.conf"], stdout=subprocess.DEVNULL)
        if args.docker_container:
            hostname = name
            prefix = ["docker", "exec", "-i"]
            if args.native:
                libraries = list((ROOT / "target/debug/build").glob("datachannel-sys-*/out/lib/libdatachannel.so"))
                if not libraries:
                    raise RuntimeError("Build native libdatachannel first")
                lib_path = "/work/" + str(libraries[0].parent.relative_to(ROOT))
                prefix += ["-e", "LD_LIBRARY_PATH=" + lib_path]
            prefix += [args.docker_container]
        else:
            hostname = command(["docker", "inspect", name, "--format", "{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}"], capture_output=True, text=True).stdout.strip()
            prefix = []
            root_der = certificate(hostname)
            command(["docker", "restart", name], stdout=subprocess.DEVNULL)
            if args.native:
                libraries = list((ROOT / "target/debug/build").glob("datachannel-sys-*/out/lib/libdatachannel.so"))
                if not libraries:
                    raise RuntimeError("Build native libdatachannel first")
                os.environ["LD_LIBRARY_PATH"] = str(libraries[0].parent)
        readiness = """import socket,sys,time
deadline=time.monotonic()+10
while True:
    try:
        connection=socket.create_connection((sys.argv[1],5349),timeout=1)
        connection.close()
        break
    except OSError:
        if time.monotonic()>=deadline: raise SystemExit('TURN TLS listener unavailable')
        time.sleep(0.1)
"""
        command(prefix + ["python3", "-c", readiness, hostname])
        protocols = ["udp", "tcp", "tls"] if args.native else ["udp"]
        if args.protocol:
            if args.protocol != "udp" and not args.native:
                raise RuntimeError("TCP/TLS requires the native backend")
            protocols = [args.protocol]
        for protocol in protocols:
            endpoint = ("turns:" + hostname + ":5349?transport=tcp") if protocol == "tls" else ("turn:" + hostname + ":3478?transport=" + protocol)
            transport = {"ice_servers": [{"urls": [endpoint], "username": "synthetic", "credential": "synthetic-laboratory-only"}], "relay_only": True, "bind_addrs": ["0.0.0.0:0"], "tls_roots_der": [list(root_der)] if protocol == "tls" else []}
            command(prefix + ["python3", "tests/integration/run.py", "--binary", args.binary, "--relay"], input=json.dumps(transport), text=True)
            print(json.dumps({"transport": "TURN/" + protocol.upper(), "synthetic_lab": "passed"}), flush=True)
        if not args.native:
            print("TURN TCP/TLS não executado: requer --native e build libnice.", file=sys.stderr)
    finally:
        subprocess.run(["docker", "rm", "-f", name], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        if connected:
            subprocess.run(["docker", "network", "disconnect", network, args.docker_container], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        subprocess.run(["docker", "network", "rm", network], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        # Remove only operator TLS material created by this invocation.
        for artifact in ["key.pem", "cert.pem", "turnserver.conf"]:
            (lab / artifact).unlink(missing_ok=True)
        lab.rmdir()


if __name__ == "__main__":
    main()
