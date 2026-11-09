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
recusa. Windows named pipes com ACL/rejeição de clientes remotos ainda precisam
de implementação e testes; o início é recusado no Windows até essa garantia
existir. Isolamento de processos mesma conta não protege de
malware equivalente.
