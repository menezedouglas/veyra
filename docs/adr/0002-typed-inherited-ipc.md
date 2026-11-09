# ADR 0002 — cofre em processo com pipes herdados

Decisão experimental em 2026-11-09, não substitui validação por plataforma.

O coordenador inicia o cofre com pipes privados herdados, desafio de bootstrap
em RAM e operações tipadas. Não há listener ou credencial em URL/argv/env/arquivo.
O daemon não recebe a autoridade nem os handles do cofre. Timeout/EOF mata a
instância e invalida a sessão, sem recuperação automática de chaves.

O daemon tem um processo próprio. Seu `VaultNetworkPort` usa uma segunda ponte
que o coordenador traduz somente a partir de um enum fechado, sem OpenOnce,
VerifyPeer ou Seal. O daemon gera o par Noise X25519 localmente e mantém a chave
privada em memória com lock verificado. Envia somente a chave pública por
BindTransport; o cofre devolve o binding assinado, preservando Ed25519/runtime.
Um segundo bind ou um bind após autorização é recusado. Ed25519 e chaves HPKE
privadas continuam no cofre; nenhuma chave privada atravessa o coordenador Tauri.
Testes de schema rejeitam o antigo PrepareTransport, respostas com noise_private
e pedidos de abertura, aprovação e envio de texto pela fronteira de rede.

Motivo: um caminho local testável e limitado reduz a superfície inicial, sem
expor assinatura/descriptografia genéricas. O JSON local limitado não é o
formato assinado entre peers; esse continua CBOR determinístico.

No Linux, `fstat` valida o tipo FIFO, UID efetivo proprietário e modo sem acesso
por grupo/outros antes do bootstrap. Essa é a verificação de credenciais/permissões
do canal herdado escolhido, sem criar um listener de Unix socket. Testes alteram
o modo de um pipe sintético e usam um descriptor de outro tipo para verificar a
recusa.

No Windows, dois named pipes unidirecionais são pré-conectados pelo coordenador
antes do spawn. Cada um usa nome aleatório, primeira instância, limite de uma
instância, rejeição de clientes remotos e DACL protegida que concede somente
direitos individuais de dados ao SID do logon atual. A máscara exclui a criação
de outra instância. O worker recebe apenas as extremidades de stdin/stdout e
valida tipo, proprietário/DACL, PID do servidor correspondente ao pai e sessão
Windows antes de ler o desafio. Os handles originais ficam não herdáveis; a
duplicação de stdio no spawn é protegida pelo mutex do Rust.

Testes nativos recusam ACL ampla, outro logon/servidor, stdio anônimo e
bootstrap parcial. O [ensaio Windows em release](../../tests/integration/windows-product-release-report.json)
exercita dois daemons e dois cofres com WSS real, Noise, mensagens HPKE nos dois
sentidos, leitura única e revogação. O daemon continua sem receber o handle do
cofre ou autoridade de abertura. Isolamento de processos da mesma conta não
protege integralmente de malware com privilégios equivalentes.
