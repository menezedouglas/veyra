# Conexão direta sem servidores

O modo padrão conecta os aplicativos por WebRTC com candidatos locais, sem
consultar sinalização, STUN, TURN, diretório de usuários ou API de IP externo.
Não exige domínio, VPS, Docker, certificado de operador ou serviço separado.
O daemon de rede e o cofre são componentes do próprio aplicativo.

## Como conectar

1. Nas duas máquinas, abra o aplicativo e clique em **Iniciar sessão**.
2. A primeira pessoa abre **Convites**, clica em **Criar convite** e entrega o
   código à segunda pessoa por um meio que ambas escolham.
3. A segunda pessoa cola o código recebido e inicia o pareamento. O aplicativo
   apresenta uma **resposta**; devolva esse código à primeira pessoa.
4. A primeira pessoa cola a resposta no mesmo campo e inicia o pareamento.
5. Em **Contatos da sessão**, ambas comparam o código de verificação por um
   canal independente e aprovam a identidade. Só então podem enviar mensagens.

Os dois códigos mantêm o prazo original de até cinco minutos. Deixe os dois
aplicativos abertos durante a troca. Uma resposta só pode ser aceita uma vez.
Se o prazo acabar ou a rede mudar, crie um novo convite; um novo pareamento
exige uma nova comparação do código. Fechar o aplicativo descarta a identidade,
convites, ofertas de chaves e mensagens pendentes daquela execução.

## Redes atendidas

O uso principal é entre computadores na mesma rede local, com tráfego entre
eles permitido. Também pode funcionar quando os endereços dos participantes
são diretamente alcançáveis. Isolamento de clientes no Wi-Fi, firewall,
sub-redes sem roteamento e NAT restritivo podem impedir a conexão.

Sem STUN/TURN não há descoberta automática do endereço público nem caminho
retransmitido. Portanto, este modo não garante comunicação entre quaisquer
duas redes da internet. Não altera firewall, modem ou permissões do sistema.
Se o sistema solicitar autorização de rede, a decisão permanece com o usuário.

## Convites e proteção

Os códigos `veyra2:` carregam a oferta ou resposta SDP completa, os candidatos
de rede, o documento público de identidade, a sala aleatória de 256 bits e o
prazo, com assinatura Ed25519. Não contêm texto de mensagem ou chaves privadas.
O formato é limitado a 16.000 caracteres para caber no IPC limitado do produto.
Eles revelam os endereços anunciados a quem receber o código. Uma assinatura
não oculta esses dados nem comprova a identidade humana.

A negociação manual usa o mesmo DataChannel confiável/ordenado com DTLS,
handshake Noise XX vinculado às identidades e envelopes HPKE do fluxo existente.
O modo manual utiliza o backend Rust/UDP tanto no Windows quanto no Linux;
o backend nativo continua disponível no modo opcional com operador.
O destinatário ainda precisa aprovar a identidade localmente. Dados de conexão
inválidos, expirados, adulterados, da própria identidade ou de outra negociação
são recusados. O aplicativo mantém o estado em RAM e não grava os códigos.
Copiá-los por ação explícita pode deixar cópias no histórico externo do clipboard.

## Operador opcional

O fluxo WSS/TURN anterior continua disponível para instalações que escolham
configurar um operador em opções avançadas ou por variáveis de distribuição.
Ele não participa do modo direto. Uma configuração externa explicitamente
informada continua sendo validada; não há troca silenciosa de operador.

`desktop:dev` inicia o modo direto por padrão. Para ensaios opcionais de WSS
local, `VEYRA_DEV_LOCAL_OPERATOR=1` habilita o helper de desenvolvimento.
Cada execução desse helper possui uma CA e um endpoint próprios. Duas
instâncias que usem WSS precisam compartilhar o mesmo operador e confiar
explicitamente em sua CA, como no laboratório de integração existente.

## Validação

O teste `direct_session_process` usa dois daemons e dois cofres reais, candidatos
somente de loopback e nenhum processo de sinalização. Exercita a troca manual
dos dois códigos, verificação, mensagens bidirecionais, leitura única e revogação.
No Windows MSVC debug, passou com atraso de **35 segundos** na devolução da
resposta, além de novo pareamento com nova comparação e aprovação do código.
Ele demonstra o produto na mesma máquina. Validação entre computadores, redes
e sistemas operacionais requer ensaios próprios; consulte `IMPLEMENTATION_STATUS.md`.

```powershell
$env:RUSTUP_TOOLCHAIN = '1.99.1-x86_64-pc-windows-msvc'
& "$env:USERPROFILE\.cargo\bin\cargo.exe" test --locked -p veyra-daemon --test direct_session_process
```

Esse fluxo funcional não encerra as pendências de instalação, acessibilidade,
auditoria de privacidade ou revisão independente da criptografia.
