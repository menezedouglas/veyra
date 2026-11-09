# Modelo de ameaças

Prioridade: autenticidade e confidencialidade, ausência de persistência,
disponibilidade, conveniência. Implementação experimental; sem auditoria.

| Adversário / falha | Controle implementado ou projetado | Limite / validação pendente |
|---|---|---|
| Rede hostil / MITM | DTLS + Noise XX, binding Ed25519, aprovação explícita e comparação independente; sem envio não verificado | A comparação humana continua necessária; validação nas plataformas e redes da matriz segue pendente |
| Sinalização maliciosa | Mensagens assinadas com vínculo de convite/papel/execução | Pode omitir, atrasar, correlacionar e interromper; infraestrutura não esconde IP |
| Relay malicioso | Apenas tráfego DTLS/Noise e envelope HPKE | Pode copiar ciphertext/metadados e negar serviço |
| Substituição de chave | BindTransport único antes da autorização, binding assinado e chave Noise remota observada; runtime novo exige confiança nova | Autossinatura não demonstra identidade humana; revogação não permite trocar o par Noise na mesma execução |
| Replay / ACK falso | Assinatura, digest, IDs, execução e dedup limitado | Cache não é persistente; restart invalida identidade e ofertas |
| Duas aberturas concorrentes | Cofre serializado e remoção de chave antes de retorno | Falha após claim causa perda intencional |
| Exaustão de memória | Tamanhos/coleções/filas/salas limitados; backpressure | Testes de carga hostil e reservas nativas WebRTC ainda necessários |
| Frontend comprometido | WebView e coordenador sem chaves privadas, comandos mínimos, validação Rust, pipes cofre separados | Pode capturar composição/leitura e solicitar ações permitidas como usuário; confirmação não é prova física de presença |
| Processo de rede comprometido | Detém sua própria chave privada Noise; BindTransport encaminha somente a chave pública ao cofre. Chaves privadas Ed25519/HPKE e autoridade ficam no cofre; ponte tipada exclui abertura, confiança e plaintext | Processos mesma conta não isolam malware equivalente; sandbox nativo e revisão ainda necessários |
| Crash / suspensão / relógio | Cofre volátil, CLOCK_BOOTTIME Linux, deadlines originais | Destrutores não são garantidos em kill/power loss; execução e IPC Windows sem validação |
| Captura / SO / GPU / teclado | Exposição mínima, esconder blur, contentProtected quando suportado, locks e dump guard verificados | Não impede destinatário de copiar, câmera, malware, swap externo, hibernação ou admins |

Fora das garantias: anonimato, segurança pós-quântica, Double Ratchet,
pós-comprometimento, deniabilidade, instalação universal Linux, entrega em toda
rede e apagamento imposto a operadores ou destinatários maliciosos.

Teste de laboratório não é prova de produção. Releases públicas dependem da
matriz de conectividade e plataformas, de endpoints e certificados reais,
auditoria de persistência e revisão independente do protocolo composto.
