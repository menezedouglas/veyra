# Protocolo experimental Veyra 1

Este documento descreve os bytes implementados, sem afirmar revisão externa.
Não há negociação de downgrade. Versão = 1; suíte = 1: HPKE
DHKEM(X25519, HKDF-SHA256), HKDF-SHA256, ChaCha20-Poly1305.

## Representação

Todo frame é array CBOR `[1, opcode, payload]`. Inteiros sem sinal usam sua
menor representação. Byte strings têm tamanho definido; não são mapas/texto
arbitrário. Arrays têm comprimentos fixos. Tags, mapas, indefinidos, dados
residuais e representações não mínimas são rejeitados: após decode limitado,
reencode deve ser idêntico à entrada. A estrutura tem profundidade fixa.

O esquema exato e a ordem de campos estão em `crates/protocol/src/lib.rs`.
Essa fonte é normativa para este código experimental. Nos frames de aplicação,
os domínios são byte strings ASCII. IDs têm 16 bytes, identidades e chaves
públicas 32, assinaturas 64.

| Opcode | Operação | Payload |
|---|---|---|
| 0 | HELLO | runtime ID |
| 1 | IDENTITY_BINDING | documento assinado |
| 2 | KEY_OFFER | oferta assinada |
| 3 | MESSAGE | envelope assinado |
| 4 | RECEIVED | ACK assinado |
| 5 | REJECTED | ID e código limitado |
| 6 | CLOSE | código limitado |

Domínio `Veyra/identity/v1`: versão, identidade Ed25519, chave pública Noise
X25519, execução e assinatura. A assinatura cobre todos os campos anteriores
com comprimento de array correspondente à ausência de assinatura.
Domínio `Veyra/key-offer/v1`: versão, suíte, remetente, destinatário, duas
execuções, ID de chave, chave pública descartável, TTL ms, política 1 e assinatura.

AAD do envelope: array de domínio `Veyra/envelope-aad/v1`, versão, suíte,
remetente, destinatário, execuções, ID de mensagem, ID de chave, TTL e política 1.
O domínio `Veyra/envelope-signature/v1` cobre versão, esse cabeçalho completo,
encapsulated key e ciphertext. A assinatura é verificada antes do claim.
O digest do envelope é SHA-256 da codificação assinada completa.

Domínio `Veyra/received/v1`: versão, ID, digest do envelope, execução receptora
e assinatura Ed25519 da identidade receptora. ACK não é recibo de leitura.

## Vínculo local do transporte

O daemon gera o par X25519 Noise e mantém a chave privada em seu próprio
`LockedBuffer`. Antes de criar convites ou autorizar peers, registra somente a
chave pública por `BindTransport { noise_public }`, encaminhado pelo coordenador
ao cofre. O cofre atualiza e assina `IDENTITY_BINDING`, preservando a identidade
Ed25519 e o `runtime_id`, e zera sua metade privada Noise inicial.

O registro é permitido uma única vez por execução do cofre. Uma autorização de
peer também encerra essa possibilidade; revogar o peer não libera novo registro.
A resposta contém apenas o documento público. O helper `prepare_transport` do
runtime lê o buffer privado local do daemon e não corresponde a uma operação IPC.

## Canal

O DataChannel permanece confiável, ordenado, binário e protegido por DTLS.
Snow usa `Noise_XX_25519_ChaChaPoly_SHA256`, sem payload de usuário no handshake
e sem 0-RTT. Prologue CBOR inclui domínio `Veyra/noise-prologue/v1`, versão,
bindings completos ordenados por papel iniciador/respondedor e SHA-256 dos
32 bytes do identificador aleatório da sala do convite.
O binding é validado contra o static remoto observado após XX. Nova conexão
precisa de handshake novo e contadores novos; `TransportState` não é reutilizado.

Código de comparação: SHA-256 de `Veyra/verification/v1\0`, comprimento de
transcript u32 big endian, transcript e representações canônicas usadas para
assinar os bindings, sem o campo de assinatura, na ordem iniciador/respondedor;
os oito primeiros bytes são apresentados em quatro blocos hexadecimais.
Aprovação humana explícita e comparação independente continuam necessárias.
Documento autossinado sozinho não identifica uma pessoa.

Após a aprovação local, cada cliente conforme envia `HELLO` com seu `runtime_id`
pelo canal Noise. Ofertas e mensagens só são aceitas depois da aprovação local e
da confirmação remota correspondente.

HPKE info é `Veyra/HPKE/X25519-HKDF-SHA256/ChaCha20Poly1305/v1`.
HPKE Base é autenticado pela assinatura externa. Nenhum nonce/KDF é implementado
manualmente. Uma oferta serve a um único envelope e a um único destinatário.
Fan-out usa ofertas e estados independentes. Não existe chave de grupo.

## Limites

Texto: 4096 bytes UTF-8, sem controles exceto LF e TAB. Frame: 16384 bytes,
ciphertext no máximo 4112 bytes, TTL até 300000 ms. Exibição até min(30000 ms,
validade restante). Até 16 peers, 16 pendências/ofertas por peer e 128 globais.
Metadados de replay vivos não são expulsos para aceitar novas mensagens.
Sem espaço, recusar com backpressure.

Oferta: prazo monotônico local original. Envelope não o estende. A duplicata
idêntica retorna ACK sem recriar pendência; digest/ID diferentes usando a mesma
oferta são rejeitados. Outro runtime exige pareamento novo. A ausência de ACK
significa entrega não confirmada.

Referências: [RFC 8949](https://www.rfc-editor.org/rfc/rfc8949.html),
[RFC 9180](https://www.rfc-editor.org/rfc/rfc9180.html),
[Noise](https://noiseprotocol.org/noise.html).
