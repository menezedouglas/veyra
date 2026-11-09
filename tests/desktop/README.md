# Teste desktop nativo

`native_smoke.py` controla o executável real por tauri-driver/WebKitWebDriver e verifica o ciclo de identidade/cofre e recusa de comandos, sem mocks. Requer Linux, Xvfb, WebKitWebDriver, tauri-driver 2.1.0 e os binários `veyra` e `veyra-vault` no mesmo diretório.

A versão Wry selecionada tem um bloqueio de automação com `incognito=true`. A execução atual **falha antes de criar uma sessão**. Veja `docs/desktop-qa.md`; não desative o modo efêmero para mascarar esse resultado.

Os testes usam somente dados sintéticos. Não forneça mensagens, convites ou identidades de usuários aos testes de captura e inspeção.

`native_accessibility.py` é a alternativa AT-SPI/X11 que preserva o binário e o modo efêmero. O teste real de início/cofre/identidade/encerramento passou 16 checks no ambiente registrado. O relatório de arquivos inclui caches e um SQLite HSTS criados pelo WebKit; não se afirma zero arquivos.

`two_desktops.py` estende a automação para duas janelas nativas, com operador WSS local e CA sintética confiada explicitamente. Não usa screenshots de conteúdo, mocks, endpoints públicos ou segredos de usuário em argumentos. Consulte o relatório efetivamente gerado antes de considerar o fluxo aprovado.

O harness fornece `VEYRA_SIGNALING_ENDPOINT` aos processos e inicia as sessões
sem preencher campos de operador na interface. O novo check de configuração
automática só constitui evidência quando constar de uma execução efetiva; os
relatórios anteriores preservados registram o fluxo da versão testada.

A execução efetiva de `two_desktops.py` passou 19 checks: WSS TLS confiável, pareamento Noise, comparação/aprovação explícita, P2P direto real e mensagens HPKE nas duas direções com abertura e ocultação por perda de foco. O relatório não contém tokens, fingerprints ou corpos. Consulte `docs/desktop-qa.md` para o escopo e os arquivos WebKit observados.
