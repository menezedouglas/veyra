# ADR 0004 — catálogo público de contatos persistente

Decisão aceita em 2026-11-10 a pedido do usuário. Substitui a proibição de
armazenar contatos da ADR 0001, preservando o estado efêmero das mensagens.

Fechar o aplicativo não deve apagar os contatos sincronizados. O coordenador
Rust mantém o catálogo separado da sessão, em `contacts.json` no diretório
nativo de dados do aplicativo (`app_data_dir` do Tauri). O arquivo usa esquema
versionado e fechado: cada contato contém somente o identificador público e o
rótulo local. A impressão digital é recalculada a partir do identificador.
Mensagens, envelopes, chaves privadas, convites, bindings, códigos de verificação,
aprovações e estado de sessão não fazem parte desse arquivo.

O catálogo é atualizado durante a sincronização, sem depender do encerramento.
As gravações substituem o arquivo de forma atômica. Falhas de leitura/gravação
são apresentadas ao usuário; arquivo inválido não deve ser substituído
silenciosamente por catálogo vazio. Revogar um contato remove sua entrada
persistida, inclusive sem sessão ativa. O acesso ao filesystem fica no módulo
nativo de contatos; a WebView não ganha permissão genérica de arquivo nem usa
storage do navegador.

Reabrir o aplicativo recupera a lista desconectada, sem códigos ou autorização
de envio. Ed25519, Noise e `runtime_id` continuam gerados por execução. Voltar a
conversar exige novo convite e verificação do transcript atual. Não há reconexão
automática nem associação de identidades distintas por nomes iguais. Persistir
um contato não comprova sua identidade humana nem restaura a confiança anterior.

Consequências: dados públicos e rótulos locais dos contatos permanecem em disco
e podem revelar relações a quem tenha acesso ao arquivo. O catálogo não recupera
mensagens perdidas nem oferece caixa postal offline. A garantia continua sendo
ausência de gravação intencional de mensagens, sem prometer ausência de vestígios
do sistema operacional ou da WebView.
