# Limites de segurança

Protocolo e composição experimental, sem revisão independente. Não usar esta
versão para comunicação de alta sensibilidade. A matriz de entrega enumera
funcionalidades e garantias ainda não implementadas ou não comprovadas.

* HPKE Base não impede replay sozinho nem protege ciphertext após comprometimento
  da chave privada receptora ainda viva. Assinaturas e estado do cofre tratam
  autenticação/replay; chaves descartáveis reduzem a retenção.
* Noise XX depende de verificar o binding e comparar identidade/transcript.
  Nenhum nome exibido, relay ou documento autossinado substitui isso.
* O catálogo público de contatos permanece em `contacts.json` no diretório de
  dados do aplicativo e pode revelar identificadores e rótulos a quem leia esse
  arquivo. A lista recuperada fica desconectada e sem confiança de sessão;
  reiniciar exige novo pareamento e verificação. Mensagens não são recuperadas.
* Zeroize atua nos buffers que controla; não limpa automaticamente cópias
  temporárias das bibliotecas, JavaScript, DOM, IPC, WebView ou GPU.
* mlock/VirtualLock podem falhar. Falha de proteção dos buffers bloqueia a
  criação de chaves/operações. Windows tem lock e exclusão WER com retorno
  verificado e compilação cruzada; seu comportamento em execução não foi validado.
* Linux configura RLIMIT_CORE=0 e PR_SET_DUMPABLE=0 com retorno verificado.
  Isso não controla processos privilegiados, snapshots, hibernação nem malware.
* Pipes herdados reduzem exposição local, mas não autenticam um binário trocado
  nem protegem contra controle administrativo ou malware da mesma conta.
  Como o canal Windows com ACL explícita ainda não existe, os workers recusam
  iniciar nessa plataforma antes de aceitar o bootstrap ou gerar a identidade.
* Uma ação `confirmed` na WebView não é prova de presença física; frontend
  comprometido pode abusar dos comandos que a própria interface pode usar.
* Timeout, falta de ACK e queda de operador produzem incerteza/perda. Não existe
  fallback para plaintext, identidade não verificada, arquivo ou fila durável.
* TURN/TLS não é HTTP. Porta 443 não implica atravessar proxies HTTP ou inspeção.
* Leitura única é uma política do cliente conforme; o destinatário pode copiar.

Não foram demonstrados anonimato, auditoria, segurança pós-quântica,
Double Ratchet, recuperação pós-comprometimento, deniabilidade, migração de
rede transparente nem compatibilidade universal Windows/Linux.
