# Veyra relay

Coturn é a implementação TURN; não há servidor de relay proprietário nem caixa
postal. A imagem 4.18.0 está fixada por digest. As alocações desaparecem no
encerramento e expiram sem refresh. As configurações não usam banco de usuários,
Redis ou estado durável. Não configurar `userdb`, nem `/dev/null`: isso ativaria
o driver SQLite. Sem a opção, o driver fica desativado (UNKNOWN/NULL).

`laboratory.conf` contém credenciais **públicas sintéticas**, aceita destinos
privados apenas na rede Docker isolada e não publica portas. O Compose de
desenvolvimento é suficiente para os ensaios UDP/TCP. `laboratory-rest.conf`
usa um segredo REST público sintético exclusivamente para testar emissão
automática pelo WSS; nunca utilizar esse segredo em produção. O script
`scripts/connectivity-lab.py --native` cria um certificado TLS sintético de um dia,
passa sua CA pública explicitamente aos peers e remove os arquivos ao terminar.
Nenhuma chave E2EE é escrita nesses arquivos. Sem `--native`, só UDP é ensaiado.

## Operação

`compose.production.yaml` exige duas configurações externas do operador:

- `VEYRA_TURN_OPERATOR_CONFIG`: arquivo protegido com `realm`,
  `static-auth-secret` de alta entropia, `cert=/run/veyra-tls/fullchain.pem` e
  `pkey=/run/veyra-tls/privkey.pem` e, se necessário, o mapeamento `external-ip`.
- `VEYRA_TURN_TLS_DIRECTORY`: certificados válidos, chave e cadeia do operador.

O entrypoint combina a referência e os segredos exclusivamente em `/run`
montado como tmpfs. Certificados/segredos operacionais são diferentes de chaves
de identidade ou de abertura dos usuários. Não embutir `static-auth-secret` no
cliente. `issue-credential.py` implementa o esquema HMAC documentado pelo Coturn,
com identificador aleatório e validade de cinco minutos; o operador ainda deve
implementar sua política de autorização/rate limit para emissão. Esse helper
nunca deve virar endpoint público irrestrito.

TURN/TLS é mapeado na porta TCP 443. Isso não o transforma em HTTPS nem garante
passagem por proxies HTTP/inspeção de rede. `no-tcp-relay` proíbe alocações de
relay TCP RFC 6062 e **não** proíbe clientes TURN conectarem via TCP/TLS para
alocações UDP, usadas pelo WebRTC.

Foram configurados limites de alocação, largura de banda, destinos internos,
multicast e TTL. Configuração de referência não substitui ensaios de carga,
proteções de borda ou capacidade de banda do operador. Software gratuito não
torna a infraestrutura gratuita.

## Logs e limites

O Coturn recebe `no-stdout-log` **e** `log-file=/dev/null`, sem syslog; os
containers usam `logging: none`, filesystem read-only e tmpfs limitado. Não são
incluídos proxy reverso, captura de pacotes ou exporter de métricas. A imagem
oficial marca o executável com capability `NET_BIND_SERVICE`; esse é o único
capability preservado para evitar `EPERM` ao executar o binário como nobody.

Antes de operar externamente, verificar também logs e retenção do host Docker,
journal, proxy eventual, DNS, snapshots e backups da infraestrutura. A inspeção
local não prova que um operador malicioso deixou de copiar ciphertext/metadados.
Esse operador não recebe chaves privadas dos usuários nem plaintext de mensagens.

Não há endpoint público configurado no repositório. Os nomes
`coturn-laboratory` e `veyra-laboratory` só existem no laboratório local.
