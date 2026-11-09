# ADR 0001 — identidade por execução e falha fechada

Decisão aceita para implementação experimental em 2026-11-09.

Ed25519, chave DH Noise X25519 e runtime aleatórios independentes são gerados
por execução. No produto, o daemon gera a chave Noise privada localmente e o
cofre assina somente seu vínculo público; o coordenador não recebe chaves privadas.
Não existe serialização privada, banco, recuperação ou contato
persistente. Arquivos de build/configuração pública não guardam estado de usuário.

Autenticidade/confidencialidade e ausência de persistência precedem entrega.
Uma garantia obrigatória indisponível recusa a operação; nenhuma camada simula
estado verificado, conectado, entregue ou lido. Infraestrutura pública fica
explicitamente não configurada até ter endpoints/certificados/política reais.

Consequências: crash pode perder mensagens aceitas, pareamento novo é obrigatório
após restart, operador não oferece caixa postal offline, e disponibilidade não
é universal. O protocolo composto precisa de revisão independente.
