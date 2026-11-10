# Privacidade, contatos salvos e estado volátil

Veyra não implementa contas, telefone/e-mail, banco de mensagens, rascunho
automático, histórico, exportação, backup ou recuperação de sessão. Preferências
visuais desta versão ficam em RAM. A aplicação não usa storage do navegador,
service worker, analytics nem fontes de CDN.

Os contatos sincronizados são persistidos pelo coordenador Rust em `contacts.json`,
no diretório nativo de dados do aplicativo (`app_data_dir` do Tauri). O arquivo
versionado contém somente o identificador público e o rótulo local de cada
contato; a impressão digital é recalculada a partir do identificador. Esses dados
podem revelar contatos a quem tenha acesso ao arquivo. Ele não contém mensagens,
chaves privadas, convites, códigos de verificação, autorizações ou estado de sessão.
Encerrar o aplicativo preserva o catálogo; revogar um contato remove sua entrada.

Confidencialidade protege o conteúdo contra intermediários; anonimato esconderia
quem se comunica. Veyra não promete anonimato. ICE/STUN/TURN e sinalização
envolvem IPs, metadados e documentos públicos de identidade. Assinaturas na
sinalização não tornam candidatos/SDP confidenciais ponta a ponta.

Buffers limitados de transporte e salas temporárias são transitórios. Não são
histórico ou caixa postal offline. Relay encaminha tráfego de sessões ativas.
Recebimento confirmado significa aceitação em RAM. Não confirma leitura e pode
ser seguido de perda do processo antes de o destinatário abrir.

A identidade existe por execução do núcleo. Reabrir uma janela não deve gerar
outra identidade se o núcleo continuar ativo; esse ciclo em background ainda
não está habilitado no desktop entregue. Reiniciar o cofre gera outra identidade
e invalida ofertas, autorizações e pendências. Os contatos salvos reaparecem
desconectados e sem autorização de envio. Não há reconexão automática: é necessário
trocar um novo convite e verificar o código da sessão atual. Uma nova identidade
remota não é associada ao contato antigo pelo nome. Ambos offline implica não entrega.

Envelopes aguardam em RAM cifrados, com prazo original. Sem oferta válida de
peer verificado, envio é recusado. Texto de composição não vira fila offline.
Após consumo o cartão deixa de existir; segunda abertura não recupera a chave.
O cliente conforme autoriza no máximo uma abertura. O destinatário pode copiar
ou fotografar conteúdo; não existe garantia criptográfica contra isso.

Operador honesto deve aplicar a política de buffers/logs da referência e conferir
o ambiente. Operador malicioso pode guardar ciphertext/metadados. A
confidencialidade do envelope não depende de ele cumprir a política de logs.

Binários, assets, configurações públicas, o catálogo público de contatos e arquivos
de TLS do operador podem existir em disco. Mensagens, chaves privadas de usuário
e estado de sessão continuam somente em RAM. Não se usa a estratégia gravar
mensagens e apagar ao sair.

A ausência de gravação intencional de mensagens pela aplicação não implica
ausência de vestígios no sistema. WebView2/WebKit, GPU, teclado, acessibilidade, clipboard
externo, swap, snapshots e hibernação podem expor dados. Incognito é uma
mitigação configurada, não evidência de zero arquivos. Auditoria de perfis do
WebView e SO por plataforma continua obrigatória antes do lançamento.
