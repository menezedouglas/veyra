# Empacotamento e releases

O workflow `Build Veyra release drafts` aceita uma tag `vX.Y.Z` e um canal.
A tag deve corresponder às versões de `package.json`, `apps/desktop/package.json`,
`apps/desktop/src-tauri/tauri.conf.json`, `[workspace.package]` em `Cargo.toml` e
`[package]` em `apps/desktop/src-tauri/Cargo.toml`. A validação ocorre antes da
compilação dos instaladores. Se a tag já existir, ela deve apontar exatamente
para o commit selecionado na execução; uma tag de outro commit é recusada.

O canal padrão do workflow, `experimental`, permite criar um rascunho de
pré-lançamento com instaladores sem assinatura enquanto houver evidências de
aceitação pendentes. As pendências continuam explícitas no relatório do job e
no rascunho; empacotar não comprova prontidão para produção.

O canal `production` exige que todos os itens de
[`release-readiness.json`](release-readiness.json) sejam `true`. Escolher esse
canal não publica a release: o resultado continua sendo um rascunho para revisão.
Os itens só devem ser atualizados após a respectiva validação, com a evidência
registrada. Os dois canais rejeitam JSON inválido, itens ausentes ou desconhecidos,
valores que não sejam booleanos e versões divergentes. O relatório de evidências
é preservado como artefato do job inclusive quando um pedido de produção é bloqueado.

Depois de passar pelo gate e pelos testes, o workflow gera um instalador Linux
`.deb` e um instalador Windows `*-setup.exe`. O rascunho recebe os dois
instaladores, `release-evidence.json` e `SHA256SUMS`, que cobre os instaladores e
o relatório. As notas incluem o canal e as pendências de aceitação. Checksums
permitem verificar os arquivos baixados; os pacotes continuam sem assinatura.

Executar novamente para a mesma tag atualiza o rascunho existente e substitui os
arquivos de mesmo nome. Uma release já publicada é recusada, sem edição ou troca
de arquivos; nesse caso, use uma nova versão. O canal experimental marca o
rascunho como pré-lançamento. A publicação permanece uma decisão de revisão.

Após enviar estas correções ao GitHub, abra **Actions → Build Veyra release drafts
→ Run workflow** e inicie uma execução nova na branch ou tag que contém o commit
corrigido. Informe a tag correspondente aos manifests e escolha o canal. **Re-run
jobs** de uma execução antiga reutiliza o commit daquela execução e não incorpora
o workflow corrigido.

Para verificar o mesmo gate localmente:

```sh
python3 scripts/release-gate.py --channel experimental --tag v0.1.1 --report release-evidence.json
python3 scripts/release-gate.py --channel production --tag v0.1.1
```

Sem `--channel`, o script mantém a política estrita de produção. `--report`
grava JSON inclusive quando o gate bloqueia a execução. `--summary CAMINHO`
acrescenta um resumo Markdown, usado com `GITHUB_STEP_SUMMARY` no Actions. O
relatório contém versões verificadas, itens concluídos, pendências, erros de
validação e o campo `ready_for_production`; `passed` informa se o canal escolhido
pode prosseguir.

As regressões do gate usam repositórios temporários e não alteram o checklist real:

```sh
python3 -m unittest discover -s tests/scripts -p 'test_*.py'
```
