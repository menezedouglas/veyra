# Privacidade e estado volátil

Veyra não implementa contas, telefone/e-mail, contatos persistentes, banco de
mensagens, rascunho automático, histórico, exportação, backup ou recuperação.
Preferências visuais desta versão ficam em RAM. A aplicação não usa storage
do navegador, service worker, analytics nem fontes de CDN.

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
e invalida ofertas, contatos e pendências. Ambos offline implica não entrega.

Envelopes aguardam em RAM cifrados, com prazo original. Sem oferta válida de
peer verificado, envio é recusado. Texto de composição não vira fila offline.
Após consumo o cartão deixa de existir; segunda abertura não recupera a chave.
O cliente conforme autoriza no máximo uma abertura. O destinatário pode copiar
ou fotografar conteúdo; não existe garantia criptográfica contra isso.

Operador honesto deve aplicar a política de buffers/logs da referência e conferir
o ambiente. Operador malicioso pode guardar ciphertext/metadados. A
confidencialidade do envelope não depende de ele cumprir a política de logs.

Binários, assets, configurações públicas e arquivos de TLS do operador podem
existir em disco. Isso é diferente de gravar mensagens, contatos, chaves privadas
de usuário ou estado de sessão. Não se usa a estratégia gravar e apagar ao sair.

A ausência de gravação intencional pela aplicação não implica ausência de
vestígios no sistema. WebView2/WebKit, GPU, teclado, acessibilidade, clipboard
externo, swap, snapshots e hibernação podem expor dados. Incognito é uma
mitigação configurada, não evidência de zero arquivos. Auditoria de perfis do
WebView e SO por plataforma continua obrigatória antes do lançamento.
