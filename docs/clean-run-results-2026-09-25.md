# Clean Run de25/09: busca de topo interrompida por política inadequada

Run `f2-forge-1790356252776`, Standard/Clean, build `069af31eb29fde958c15fbb1520b5b3e02f8bf31-dirty-523203e29c2f`,
GPU RTX3060Ti, driver616.92, contratos Discovery7/Frontier29/Apply32. Inspeção somente leitura;
nenhum Resume, reset, reconhecimento ou carga nova. Snapshot e hashes:
`target/beta/clean-run-review-20260925/`. Checkpoint pausado após74,36min,5/24 admissões.

## Evidência

Stock sustentado1740MHz, boost observado1920MHz, limite200W. A busca inicializou as bandas
em1740/1665/1575MHz (100/95/90% do stock, arredondados a bins), antes de descobrir o topo.
Nenhuma das25 observações testou clock acima de1740MHz. Não há falha de integridade/TDR nas
25 observações; todas têm restauração e BootFlag limpos. Isso não é auditoria do Event Log.

| Par | PowerRender p99 | Matriz completa | Resultado da busca |
|---|---:|---|---|
|1740@937|198,406W|Sim|Primeira base de desempenho|
|1665@893|179,622W|Sim|Primeira base intermediária|
|1575@850|164,774W|Sim|Primeira base econômica|
|1740@931|192,321W|Não|DX11 sem exposição suficiente; banda fechada|
|1665@887|178,616W|Não|Parada manual durante Texture; banda indevidamente fechada|

O DX11 de1740@931 acumulou25.946ms no alvo para30.000ms exigidos, em73.442ms ativos.
Não registrou excesso do teto, erro silencioso, perda do dispositivo ou throttle térmico;
pico200W/p99199,84W, p51635MHz.1740@937 havia acumulado36.893ms no alvo e passado,
também com pico200W/p51635MHz. A deficiência de exposição não comprova instabilidade nem
fronteira do hardware. Tampouco autoriza aprovar o par ignorando os4.054ms ausentes.

1665@887 passou Discovery, Texture curto e DX11. Texture completo terminou com
`workload_cancelled`/`inconclusive_reason=cancelled`, sem erro físico, mas chegou ao scheduler
como Inconclusive e fechou a banda. O checkpoint geral ficou paused/Resume disponível.
Portanto Resume já perderia essa faixa por uma classificação incorreta do cancelamento.
Nenhum perfil final foi publicado (godforge/brokkrs/deep_calm nulos).

## Causas identificadas no código

1. `f2_qualified_search_seeds` ancora a banda principal no p5 stock; as econômicas começam
   imediatamente a95/90% dessa base, não do topo descoberto.
2. `qualified_search::record(Qualified)` tenta menor tensão antes de maior clock na banda
   principal. `record(Inconclusive)` fecha toda a banda. Assim um ponto não exercitado
   elimina também alternativas superiores ainda não testadas. Não foi falta do orçamento8h.
3. A matriz repete exposição completa por candidato antes de descobrir qual região merece
   exploração, investindo cedo em clocks econômicos cujo intervalo final ainda é desconhecido.
4. O gate devolve Inconclusive para `workload_cancelled`; o resultado perde a distinção entre
   operador parando e teste incapaz de qualificar. Erros físicos devem manter precedência,
   mas cancelamento sem erro não deve virar evidência contra a região.

A alteração de24/09 manteve verificações de segurança úteis, mas mudou o objetivo de busca.
O aumento do tempo de teste aceito pelo usuário não significava aceitar substituir a descoberta
primeiro do topo por três bandas locais partindo do stock. O desenho deve ser corrigido.

## Direção da revisão — ainda não implementada

- Contrato reafirmado pelo usuário: descobrir primeiro o maior clock sustentável qualificado
  dentro do limite de potência da GPU; depois explorar os três objetivos em[0,90*Cmax,Cmax].
  Os10% delimitam a região; não fixam os três perfis artificialmente em100/95/90%.
- Separar três resultados: integridade/estabilidade, clock efetivamente exercitado e potência
  da carga de comparação. Power-bound e exposição insuficiente não são falhas físicas nem
  prova de que todo clock maior seja inviável.
- Priorizar uma busca conjunta de clock/tensão para o topo, mantendo a melhor base qualificada.
  Uma recusa local não encerra automaticamente todas as alternativas dessa faixa; transições
  devem depender do motivo e de uma agenda finita, sem repetir o mesmo teste indefinidamente.
- Revisar a capacidade da carga de acumular exposição no alvo com clock contido: manter a
  exigência de evidência, diagnosticar/adaptar a carga em janela limitada; não baixar30s só
  para aprovar o resultado observado. Calibração de potência e stress integral têm papéis distintos.
- Só depois do topo estabelecer a faixa econômica. Escolher Godforge/Brokkr/DeepCalm entre
  pares realmente medidos, sem usar1800@875 como semente/resultado obrigatório.
- Preservar stock oracle, concorrência, integridade, contenção, transações e recuperação.
  Corrigir separadamente o cancelamento. Não começar por reescrever shaders ou zerar proteções.
- Quando limite de busca acabar antes de delimitar o topo, declarar melhor ponto encontrado /
  descoberta incompleta, em vez de representar teto do algoritmo como teto da GPU.

## Critério confirmado pelo usuário

O usuário escolheu: **todas as cargas, inclusive o stress mais pesado, precisam sustentar
o clock sem atingir o limite de potência**. A recomendação inicial de usar somente uma carga
representativa como envelope foi rejeitada.200W é o limite desta placa, não constante global.

Isso invalida como prova do novo objetivo a aprovação baseada apenas em janela de exposição
reduzida. Em1740@937, aprovado pelo contrato atual, a primeira fase DX11 com duty100% teve
máximo ativo1680MHz, embora fases seguintes tenham alcançado1740. Portanto o critério atual
não demonstra1740 sustentável em toda a matriz. O boost stock1920 também não é essa prova.
O novo teto pode ficar abaixo de um clock estável em jogos; nenhum1800/1890/1920 é prometido.

Direção concreta do método:
1. Estabelecer referência stock e caracterizar quais cargas impedem clock sustentado ou
   acionam limitação de potência. Separar carga ativa das pausas intencionais; registrar
   tensão, clocks e potência por fase com precisão suficiente para distinguir limite real
   de potência máxima arredondada na exportação.
2. Buscar primeiro o topo numa sequência de candidatos de clock/tensão, usando as cargas
   limitantes para rejeição inicial e a matriz completa para certificar avanço. Uma exposição
   a duty reduzido não substitui sustentação sob carga integral. Falha por energia exige buscar
   redução de consumo; insuficiência de evidência não certifica nem condena a região inteira.
3. Só chamar um par de topo qualificado se as fases ativas obrigatórias demonstrarem o clock,
   o envelope energético e a integridade, com cleanup. Separar melhor conhecido de fronteira
   delimitada. Ao encerrar por orçamento com alternativas desconhecidas, declarar incompleto.
4. A partir desse topo, medir candidatos dentro dos10% inferiores e escolher os três perfis
   por seus objetivos, submetendo TODOS ao mesmo critério de pior carga. Menor clock/tensão
   nunca herda aprovação automática.
5. Validar a implementação offline com esta run:1740@937 não pode representar topo sustentado
   por todas as cargas;1740@931 não prova instabilidade;1665@887 cancelado não fecha região;
   nenhuma banda econômica nasce antes da etapa de topo; orçamento não fabrica convergência.

Não reintroduzir um corte arbitrário198W. Definir o gate a partir do limite real, telemetria
precisa e motivo de limitação. Preservar a matriz/stock oracle e recuperação enquanto se muda
o planejador e o contrato de aprovação. Nenhuma alteração de algoritmo foi feita nesta investigação.

## Esclarecimento posterior: teto rígido, clock inferior permitido

O usuário confirmou que quedas de clock quando a carga não exige o alvo são aceitáveis;
qualquer excursão acima do teto (inclusive um bin de15MHz) compromete a atribuição do teste.
Isso não revoga o requisito de sustentação no pior stress: distinguir pausa/baixa demanda de
incapacidade de sustentar o alvo sob carga ativa exigente. Não exigir clock constante em idle.

Antes da busca, verificar capacidade de conter clock/tensão durante carga contínua e transições.
Um resultado nominal1800@875 não certifica esse par se houve1815MHz ou tensão efetiva superior.
Falha de contenção deve ficar separada de instabilidade atribuída ao par: bloquear aprovação e
preservar recuperação/incidente, sem atribuir automaticamente o defeito físico a1800@875.
Sem atingir suficientemente o alvo, há prova insuficiente; médias/percentis não podem esconder
excursões superiores. Picos transitórios não amostrados não podem ser declarados inexistentes.

Separar descoberta de limite operacional e validação de integridade com simulação de jogo:
exigir ambas no par final. Picos observados de tensão não são por si prova da tensão necessária
ou de estabilidade; validar a tensão realmente usada durante a exposição ao clock.
Nenhuma mudança de código ou execução de carga realizada neste esclarecimento.
