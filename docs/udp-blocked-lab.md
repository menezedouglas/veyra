# Prova com UDP bloqueado no cliente

Em 2026-11-09, o mesmo cliente Linux descartável manteve DROP de UDP durante
três ensaios consecutivos. TURN/UDP falhou no transporte; TURN/TCP e TURN/TLS
completaram duas instâncias reais, rota Relay, Noise XX, HPKE, ACK idempotente,
troca bidirecional e abertura única. Só o DNS interno do Docker teve exceção.

| Ensaio | Resultado observado | DROP IPv4 OUTPUT acumulado |
| --- | --- | --- |
| TURN/UDP, controle negativo | exit 1; ambos `wait_open: Timeout` | 4 pacotes / 272 bytes |
| TURN/TCP | exit 0; todas as asserções do fluxo passaram | 9 pacotes / 852 bytes |
| TURN/TLS | exit 0; todas as asserções do fluxo passaram | 14 pacotes / 1.432 bytes |

As regras permaneceram instaladas entre os ensaios. No final, a exceção DNS
contava 36 pacotes / 3.289 bytes em INPUT e OUTPUT; o DROP IPv4 INPUT ficou em
zero. As regras IPv6 DROP também permaneceram instaladas, com contadores zero:
os caminhos de relay exercitados eram IPv4, não uma prova de rede IPv6 real.

## Isolamento e artefatos

O cliente foi criado de uma imagem de laboratório baseada em Debian bookworm-slim
com digest `sha256:734a72b3d6142a71993b4bc580ca01d8ca687f77a2c422d96a961d3376b179bc`.
O Dockerfile usado para construir o cliente está em
`tests/connectivity/udp-blocked-client.Dockerfile`. O cliente tinha Python
3.11.2-1+b1, libnice 0.1.21-1, libssl3 3.0.22-1~deb12u2, iptables 1.8.9-2
com backend nf_tables e ca-certificates 20250419~deb12u2. `node`, `rustc` e
`cargo` estavam ausentes. Nenhuma compilação Rust foi executada nesta prova.

O `docker inspect` confirmou `Privileged=false`, `CapDrop=[ALL]`, somente
`CapAdd=[CAP_NET_ADMIN]`, rede bridge privada de laboratório, rootfs read-only,
repositório montado read-only, 256 MiB de memória e 64 processos/threads.
`/tmp` e `/run` eram tmpfs limitados. O namespace observado era
`net:[4026533324]`; não foi usado `--network host` nem `--privileged`.
Todos os comandos iptables foram executados por `docker exec` nesse cliente.
As redes descartáveis foram administradas pelo Docker; nenhuma regra manual de
DROP foi aplicada ao host, ao builder ou aos relays.

Binário de integração `target/debug/veyra-tests`, SHA-256 antes e depois:

```text
4a74df4ef5ff58b6a1703cb1d89c56464625dd20b2f027e924e3222f26120288
```

Biblioteca `libdatachannel.so` efetivamente selecionada pelo harness, SHA-256:

```text
2d67d1c994fd4a5023283b738f9575af05e037563d9d87c7209d7c26f022072c
```

O backend era `datachannel 0.16.1`, libdatachannel 0.23.2 com libnice, mais o
adaptador TLS rustls descrito no ADR 0003. O Coturn era 4.18.0, fixado pelo digest
`sha256:bbefd3e1fdfdc0d58770fe01b581fd8b00d9f3a5580d00acb77cf719a6bc78e3`.
Cada ensaio usou um relay e uma rede próprios, sem portas publicadas.
Os certificados eram sintéticos, correspondiam ao hostname exclusivo do relay
e sua CA pública era fornecida explicitamente aos peers. Não foi desabilitada a
validação TLS. Dados e credenciais usados eram exclusivamente sintéticos.

## Reprodução

Preparar previamente `veyra-tests` com a feature
`veyra-transport/native-libnice` e o toolchain CMake documentado. O cliente de
teste só recebe o binário e as bibliotecas já compiladas. A capability de rede é
exclusiva deste laboratório; não faz parte do aplicativo distribuído.

```sh
docker build -f tests/connectivity/udp-blocked-client.Dockerfile \
  -t veyra-udp-blocked-client-lab tests/connectivity

veyra_lab_client=veyra-udp-blocked-client
veyra_lab_network=veyra-udp-blocked-base
docker network create "$veyra_lab_network"
docker run -d --name "$veyra_lab_client" --network "$veyra_lab_network" \
  --cap-drop ALL --cap-add NET_ADMIN --security-opt no-new-privileges \
  --read-only --tmpfs /tmp:rw,noexec,nosuid,size=4m \
  --tmpfs /run:rw,noexec,nosuid,size=4m --memory 256m --pids-limit 64 \
  --mount "type=bind,src=$PWD,dst=/work,readonly" --workdir /work \
  veyra-udp-blocked-client-lab

docker exec "$veyra_lab_client" iptables -w -A OUTPUT -p udp \
  -m conntrack --ctorigdst 127.0.0.11 --ctorigdstport 53 -j ACCEPT
docker exec "$veyra_lab_client" iptables -w -A INPUT -p udp \
  -m conntrack --ctorigdst 127.0.0.11 --ctorigdstport 53 -j ACCEPT
docker exec "$veyra_lab_client" iptables -w -A OUTPUT -p udp -j DROP
docker exec "$veyra_lab_client" iptables -w -A INPUT -p udp -j DROP
docker exec "$veyra_lab_client" ip6tables -w -A OUTPUT -p udp -j DROP
docker exec "$veyra_lab_client" ip6tables -w -A INPUT -p udp -j DROP
```

O DNS do Docker sofre DNAT para uma porta interna efêmera antes do filtro. Por
isso, a exceção usa o destino/porta **originais** do conntrack, em ambos os
sentidos. Uma tentativa inicial com exceção incompleta parou no DNS antes de
exercitar ICE e foi excluída da evidência. Depois da correção, os contadores
foram zerados antes dos três ensaios apresentados na tabela.

```sh
python3 scripts/connectivity-lab.py --native \
  --docker-container "$veyra_lab_client" --protocol udp
# Esperado: exit 1 em wait_open e incremento do DROP, não falha de DNS/loader.

python3 scripts/connectivity-lab.py --native \
  --docker-container "$veyra_lab_client" --protocol tcp
python3 scripts/connectivity-lab.py --native \
  --docker-container "$veyra_lab_client" --protocol tls

docker exec "$veyra_lab_client" iptables -n -v -x -L OUTPUT
docker exec "$veyra_lab_client" iptables -n -v -x -L INPUT
docker exec "$veyra_lab_client" ip6tables -n -v -x -L OUTPUT
docker exec "$veyra_lab_client" ip6tables -n -v -x -L INPUT

docker rm -f "$veyra_lab_client"
docker network rm "$veyra_lab_network"
```

Se o ambiente negar NET_ADMIN, o ensaio deve ser registrado como bloqueado;
não é necessário nem autorizado compensar isso alterando o firewall do host.
O helper limpa os relays, redes e fixtures TLS de cada invocação. O cliente e
sua rede base também foram removidos ao concluir a execução observada.

## Alcance

Esta é uma prova de transporte de dois processos de integração com relay
forçado, sob bloqueio real de UDP no namespace do cliente. Não cobre GUI,
instalador, custódia/IPC dos processos de produto, Windows, NAT de operadora,
proxies HTTP, inspeção corporativa ou migração de rede. O trecho relay–peer do
Coturn continua usando UDP normalmente; o trecho cliente–TURN testado usa
TCP/TLS, conforme exigido. Não foi feita captura de pacotes. O aviso GLib de
`consent-freshness` já documentado apareceu e permanece uma limitação separada.

Após este ensaio, foi corrigida a conversão de C strings no binding Rust
`datachannel` (veja ADR 0003). O helper novo, SHA-256
`9dc6316c4967fe848d34a8f992b6a7e20719e65c7c37b827d84ced6f36d0cba6`, passou
novamente em direto e TURN/UDP/TCP/TLS, com a mesma biblioteca C++ de hash
`2d67d1c994fd4a5023283b738f9575af05e037563d9d87c7209d7c26f022072c`.
As regras de firewall não foram repetidas nessa rodada; a tabela deste documento
continua vinculada ao ELF anterior explicitamente registrado acima.
