# Contribuir

Leia [a especificação](docs/requirements.md), o modelo de ameaças, protocolo,
ADRs e matriz de entrega. Preserve a prioridade autenticidade/confidencialidade,
estado volátil, disponibilidade e conveniência.

Use Rust fixado por `rust-toolchain.toml`, Node pela `.node-version` e pnpm pela
versão em `package.json`. Instale com lockfile congelado. Dependências novas
precisam de documentação primária, compatibilidade, licença e lockfile atualizado.

Execute `cargo fmt --all -- --check`, `cargo clippy --locked --all-targets -- -D warnings`,
`cargo test --locked`, `pnpm test`, `pnpm build` e os testes de laboratório
relevantes. Não descreva teste não executado como aprovado. Não use mocks como
evidência do fluxo distribuído. Use somente mensagens sintéticas em testes de
persistência, screenshots, dumps e tráfego.

Não adicione logs com conteúdo, segredos, tokens, IPs de peers ou SDP, storage
do browser, filas duráveis, analytics, shell ou rede arbitrária na WebView.
Mudanças de protocolo/custódia precisam atualizar documentação e testes de
adulteração, replay, prazo e concorrência. Contribuições sob Apache-2.0.
