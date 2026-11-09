# ADR 0003 — Prova de compatibilidade WebRTC e seleção de backend

Data: 2026-11-09. Estado: experimento validado parcialmente; liberação desktop bloqueada.

O transporte precisa de DataChannel nativo binário, confiável e ordenado,
DTLS ativo, caminho direto, relay TURN/UDP e cliente TURN/TCP/TLS. A rota
apresentada deve vir do par ICE selecionado; a presença de um candidato relay
no gathering não prova que ele está em uso.

## Evidência e decisão

`webrtc = 0.21.1` foi obtido do registro oficial e compilado com Rust 1.99.1.
Dois processos reais trocaram dados sintéticos nos dois sentidos em caminho
direto e pelo Coturn 4.18.0 em UDP. O fluxo de integração também troca frames
Noise, ofertas/envelopes HPKE, ACK e abertura única.

O código oficial `src/peer_connection/transport/turn_relayer.rs`, na versão
0.21.1, recusa URLs seguras e protocolos diferentes de UDP (`Skipping
unsupported secure TURN url` e `Skipping unsupported non-UDP TURN url`). A
linha anterior `webrtc-ice 0.17.3` também deixa TCP/TLS como TODO. O adaptador
Veyra transforma essa ausência em erro explícito, em vez de permitir que a
biblioteca ignore silenciosamente uma configuração necessária.

Foi implementado um adaptador alternativo por `datachannel = 0.16.1`, com
`datachannel-sys = 0.23.0+0.23.2` e a implementação existente libdatachannel
0.23.2, compilada com `USE_NICE=ON`, libnice 0.1.21 e OpenSSL 3.0.22. A feature
`veyra-transport/native-libnice` preserva a mesma API da aplicação. O backend
padrão libjuice também não implementa TURN/TCP/TLS e não serve como substituto.
O arquivo CMake em `infrastructure/development` exige libnice explicitamente.
Esse backend alternativo já passou a prova de dois processos em direto e
TURN/TCP e TURN/TLS com o adaptador abaixo. Ele ainda requer empacotamento das bibliotecas nativas e validação
Windows; não é habilitado silenciosamente como backend desktop de produção.

## Lacuna TLS que um teste ingênuo esconderia

Na libnice 0.1.21, `agent_create_tcp_turn_socket()` não estabelece TLS em
compatibilidade RFC: `NICE_RELAY_TYPE_TURN_TLS` só seleciona pseudo-SSL nos
modos legados Google/MSOC. O código upstream consultado mantém essa estrutura.
Além disso, o Coturn pode reconhecer TCP claro na sua porta de TLS. Portanto,
uma troca bem-sucedida usando apenas uma URI `turns:` **não comprova TLS**.
A composição Veyra resolve essa lacuna com um adaptador de byte stream usando
`tokio-rustls 0.26.6` e `rustls 0.23.46`: certificado, hostname/SNI e validade
são verificados antes de abrir uma porta efêmera somente em 127.0.0.1. Libnice
usa TURN/TCP nessa porta; o adaptador encaminha o stream por TLS ao Coturn.
Não há implementação própria de TURN/WebRTC nem modo sem validação. Há no máximo
oito conexões por endpoint, quatro ativas, buffers de 16 KiB por sentido e
encerramento das tarefas quando o peer é descartado. Falha de autenticidade
recusa a criação da conexão. Indisponibilidade TCP permite tentar somente as
outras rotas configuradas ou o P2P permitido pela política.

Três testes passaram: handshake TLS real observado pelo cliente e servidor,
CA não confiável recusada e hostname incorreto recusado. O fluxo de dois processos
Noise/HPKE/ACK/leitura única passou pelo Coturn real em TURN/UDP, TURN/TCP e
TURN/TLS. O primeiro certificado de laboratório foi recusado por ser CA usado
como certificado de servidor; foi corrigido para CA:FALSE, sem reduzir a
validação. Isso também impede chamar de TLS uma URI que só selecionou TCP.

`tls_roots_der` contém apenas certificados públicos de operadores explicitamente
configurados localmente; não devem vir de convites, SDP ou sinalização. A seleção
normal usa a trust store do sistema. A feature nativa é opt-in e o guard de build
exige o toolchain CMake com USE_NICE=ON. Bibliotecas nativas, licenças e caminhos
de carga ainda exigem empacotamento e testes por plataforma.

A imagem final foi atualizada de 4.6.3 (somente prova inicial local) para
`coturn/coturn:4.18.0` pelo digest
`sha256:bbefd3e1fdfdc0d58770fe01b581fd8b00d9f3a5580d00acb77cf719a6bc78e3`.
Os três caminhos foram repetidos com sucesso nessa imagem. A revisão dos
advisories oficiais identificou correções posteriores à versão inicial, incluindo
[GHSA-4v97-rxjj-4f99](https://github.com/coturn/coturn/security/advisories/GHSA-4v97-rxjj-4f99)
e [GHSA-m562-mf7x-q7rr](https://github.com/coturn/coturn/security/advisories/GHSA-m562-mf7x-q7rr).
Isso não é uma varredura completa de vulnerabilidades da imagem e seus pacotes.

## Limites observados

O relatório de estatísticas `get_stats` não forneceu IDs resolvíveis no primeiro
teste. A implementação usa agora o getter real de par selecionado do transporte
ICE, sem registrar endereços. A alternativa nativa usa os candidatos do par
selecionado do C API. Somente `Direct`, `Relay` e `Unknown` atravessam a API UI.

Foi encontrado um defeito concreto no binding `datachannel 0.16.1`: o getter
aloca dois buffers com o tamanho do maior candidato, mas a conversão Rust
original exige que o único NUL seja o último byte. Quando os candidatos têm
comprimentos diferentes, o NUL seguido de padding do candidato menor faz o
binding devolver `None`, mesmo com o DataChannel funcionando. Não é evidência
de uma rota realmente desconhecida nem de atraso do tick de um segundo.
O getter libnice 0.1.21 verifica a existência do par selecionado sem exigir
explicitamente o estado `READY`; uma ausência transitória continua possível,
mas não explica esse erro persistente de conversão.

O patch local em [vendor/datachannel](../../vendor/datachannel/README.veyra.md)
preserva a distribuição 0.16.1 e troca somente a leitura da C string por
`CStr::from_bytes_until_nul`, com o mapeamento do erro existente e regressões
para buffers de candidatos com comprimentos diferentes, ausência de terminador
e UTF-8 inválido. Não é atribuída uma correção upstream. A origem, checksum e
licença original estão preservados. A regressão de comprimentos diferentes
falhou com o helper original; os três casos passaram com a correção, assim como
os testes TLS nativos e Clippy. O pacote foi reconstruído e instalado incluindo
os fontes modificados; as provas instaladas direta e autoTURN UDP passaram
com rotas reais `Direct` e `Relay`. Veja [testing.md](../testing.md).

Os ensaios são locais, sintéticos e em uma rede Docker isolada. A prova adicional
anterior ao patch com [UDP bloqueado no cliente](../udp-blocked-lab.md) recusou TURN/UDP e completou
TURN/TCP e TURN/TLS sob as mesmas regras DROP, com contadores registrados.
Não comprova travessia de NAT de operadoras, inspeção corporativa, mudança de
rede, migração, interoperabilidade Windows/Linux ou operação externa.
Os avisos GLib de propriedade `consent-freshness` no backend nativo foram
observados e precisam ser resolvidos antes de um lançamento de produção.

## Fontes primárias consultadas

- [webrtc-rs e versões](https://github.com/webrtc-rs/webrtc)
- [API webrtc 0.21.1](https://docs.rs/webrtc/0.21.1/webrtc/)
- [Binding datachannel 0.16.1](https://docs.rs/datachannel/0.16.1/datachannel/)
- [Build oficial datachannel-sys](https://github.com/lerouxrgd/datachannel-rs/blob/master/datachannel-sys/build.rs)
- [ICE libdatachannel](https://github.com/paullouisageneau/libdatachannel/blob/master/src/impl/icetransport.cpp)
- [libnice 0.1.21, agent.c](https://github.com/libnice/libnice/blob/0.1.21/agent/agent.c)
- [Coturn: tipos de listener, autenticação, quotas e logging](https://github.com/coturn/coturn/blob/master/examples/etc/turnserver.conf)
