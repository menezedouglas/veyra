# Proveniência de avisos originais preservados

`scripts/generate-rust-notices.py` resolve metadados com Cargo `--locked`, todas
as features e filtro Linux x86-64. Percorre dependências normais e de build dos
roots desktop, daemon e sinalização, excluindo arestas de desenvolvimento.
Isso pode incluir componentes opcionais além do binário de uma configuração
particular; não é um SBOM mínimo nem uma auditoria de compatibilidade jurídica.

Os textos originais são copiados integralmente para `docs/THIRD-PARTY-RUST.txt`
e `docs/THIRD-PARTY-NATIVE.txt`. O relatório JSON registra contagens e exceções.
As fontes publicadas de 21 subcrates não incluíam arquivo de licença/aviso:

- Para a família RTC e `libappindicator-sys`, foram preservados os arquivos da
  raiz do repositório, encontrados em outro pacote resolvido com o mesmo commit
  em `.cargo_vcs_info.json`. O relatório identifica cada proveniência.
- `rust-upstream/` preserva arquivos originais obtidos dos commits exatos para
  alloc-stdlib, asn1-rs-impl e dlopen2/derive. Cada `SOURCE.json` contém URL,
  revisão e hash do texto. O gerador verifica esses hashes.
- O snapshot de selectors não continha LICENSE. Foi preservado seu comentário
  inicial original e o texto MPL 2.0 canônico da Mozilla explicitamente
  referenciado por esse comentário e pelo SPDX do pacote. Isso está identificado
  como material referenciado, sem inventar um LICENSE existente no snapshot.
- `nlohmann-json-v3.12.0` corresponde ao gitlink `deps/json` do libdatachannel
  v0.23.2. O texto MIT original foi preservado embora JSON pertença aos exemplos
  excluídos por `NO_EXAMPLES=ON` na build observada.

O bundle nativo também inclui os copyrights disponíveis dos pacotes da
distribuição Linux encontrados na ligação dinâmica de libdatachannel e os
textos completos de `/usr/share/common-licenses` referenciados por eles. Esses
avisos não constituem evidência de dependências/licenciamento do Windows.

Reprodução, no ambiente de desenvolvimento com Cargo e suas fontes disponíveis:

```sh
python3 scripts/generate-rust-notices.py
```

O comando não compila Rust nem executa o aplicativo. Não lê mensagens, chaves,
credenciais ou arquivos de runtime. Preservar avisos não decide obrigações de
distribuição, disponibilidade de fontes ou compatibilidade de licenças; a
revisão de liberação deve conferir essas obrigações por pacote e plataforma.

## Fonte local modificado de datachannel

O fonte modificado de `vendor/datachannel` acompanha o binário em
`datachannel-0.16.1-veyra-source.tar.gz`, incluído como resource nas configurações
Tauri release e native-linux. Sua disponibilidade no pacote não depende de um
repositório remoto publicado.

Após concluir alterações no vendor, gere o arquivo local com:

```sh
python3 scripts/package-datachannel-source.py
```

O script preserva os bytes dos fontes, manifests normalizado e original,
`LICENSE`, `.cargo_vcs_info.json`, metadados `.github`/Git e `README.veyra.md`,
excluindo diretórios de build/cache e o banco de objetos `.git`. A ordem das
entradas, donos, permissões, timestamps tar e cabeçalho gzip são normalizados.
O empacotamento recusa arquivos obrigatórios ausentes, symlinks e mudanças do
vendor durante a leitura. O archive deve ser regenerado após mudar o patch e
antes de empacotar o aplicativo; não compila nem executa o produto.

Incluir esse fonte e os avisos preservados descreve o conteúdo distribuído; não
é uma conclusão de auditoria jurídica ou de compatibilidade de licenças.
