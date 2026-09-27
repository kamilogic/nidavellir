# Perfis iguais após a Clean Run — análise de 2026-09-17

## Resultado confirmado

Run `f2-forge-1789668446810`, das15:07:26 às17:15:28 locais, aproximadamente2h08.
Build das observações: `069af31eb29fde958c15fbb1520b5b3e02f8bf31-dirty-68d17580649e`.
Godforge, Brokkr's e Deep Calm são cópias de **1710 MHz @868 mV**, p99 final196.668W.
862mV no campo `voltage_mv` é a fronteira anterior à margem; o Apply qualificado é868mV.
Os quatro backends passaram nesse par. A publicação não inventou a qualificação, mas a
conclusão de três perfis satisfatórios excede o que a exploração demonstrou.

72 observações:32 Discovery (28 validadas/4 limitadas por potência),24 Frontier validadas,
16 Apply (13 validadas/3 inconclusivas). Nenhum erro físico, blacklist nova ou falha de
recuperação registrado; temperatura máxima77°C. Treze etapas DX11 consumiram91min de carga.

| Par de Apply | p99 DX11 | Exposição exata ativa | Resultado DX11 | Destino |
|---|---:|---:|---|---|
|1890@943|199.796W|21.185s|Inconclusivo|Potência e exposição insuficientes|
|1860@931|200.015W|31.232s|Validado|Excluído por potência|
|1800@906|200.018W|41.108s|Validado|Excluído por potência|
|1725@875|199.441W|77.321s|Validado|Excluído por potência|
|1710@868|196.668W|77.223s|Validado|Passou também Vulkan, DX12 e Endurance|

Valores detalhados de todos os pares: `target/beta/profile-selection-20260917/analysis.json`.
As treze etapas DX11 registraram `upper_clock_exceeded=false`. Dez passaram seus critérios
de integridade/exposição; nove dessas dez foram eliminadas somente pelo teto energético.
As três inconclusivas (1875,1890,1830) também excederam esse teto. Logo, remover a recusa
por residência não teria recuperado esses perfis sob a política energética atual.

## 1. A busca não representa uma GPU sem histórico

A auditoria da autorização contém três incidentes antigos usados pelo cone TDR:
1920@931,1860@900 e1920@943. O último registro diz que houve reconciliação de reinício
com candidato armado; sua nota não apresenta um novo evento Windows TDR. Isso exige
revisão da atribuição, não prova que o registro é falso nem autoriza apagá-lo.

`f2_effective_candidate_crashes` inclui CandidateCrash rígidos de contrato29 ou posterior.
`rigid_tdr_safety_cone_floors` projeta cada evento para clocks inferiores, relaxando somente
um bin de tensão por bin de clock. A descida para antes de executar pares nessa região.
Essa projeção é uma restrição conservadora de busca, não uma medição de instabilidade de
cada par excluído. A fronteira salva não distingue adequadamente mínimo físico encontrado
de último ponto permitido pela política.

Exemplo coerente com a regra e os bins desta run: de1920@943 até1800, oito passos de15MHz
projetam a sequência943→937→931→925→918→912→906→900→893mV. O primeiro bin acima é900mV;
a descoberta chegou a1800@900 e Apply acrescentou um bin de margem, resultando em1800@906.
1800@875 não foi testado em nenhuma etapa. A enumeração completa da curva e os motivos
individuais de parada não sobreviveram no log final; a explicação do cone vem do código,
histórico de entrada e sequência observada, e não de um log completo do scheduler.
Outros incidentes também projetam restrições sobre1800MHz: revisar apenas o registro943mV
não torna875mV automaticamente permitido. O problema é avaliar a atribuição e o alcance
da inferência sem confundi-los com ensaios físicos de cada ponto.

O manifesto confirma: Clean Run descartou positivos e preservou condenações/cone. Portanto,
há medições novas, mas o domínio de busca continua condicionado ao histórico desta GPU.
Não se pode concluir desta run que1800@875 falha, nem que a mesma curva seria encontrada
num usuário novo sem esses incidentes. Nenhum negativo foi apagado nesta investigação.

Referências: `crates/core/src/condemnation.rs:244`,
`crates/service/src/gpu_power_sweep.rs:314`, `crates/service/src/gpu_undervolt.rs:7025`.

## 2. O critério energético domina a seleção dos três objetivos

O código admite candidatos até200W, mas exige pior p99 de Apply até99% desse limite:
198W, igualmente para todos os perfis. Todos os12 pares acima de1710 excederam198W.
Assim, Godforge foi reduzido até caber nessa regra, inclusive quando o DX11 tinha passado.
Isso é recusa por política energética, não evidência de instabilidade do clock/tensão.

A triagem anterior não antecipa bem essa recusa. No MESMO par1710@868:
Discovery PowerRender171.867W; Apply DX11196.668W; Vulkan173.618W;
DX12174.285W; Endurance176.636W. Não se trata apenas de ter subido a tensão pela margem:
o contraste acima usa o mesmo par elétrico. Cargas diferentes produzem métricas diferentes.
O scheduler gastou sete minutos em cada um dos12 pares rejeitados antes de passar ao próximo.

O seletor pontua MHz/pior-p99, substituindo depois da qualificação a medição de triagem
pelo pior p99 de Apply. Não mede diretamente ganho de desempenho/eficiência em jogos.
Também não é correto comparar os1755MHz/199.659W do baseline PowerRender com1710MHz/196.668W
do DX11 como se fossem a mesma carga e derivar daí uma perda real de FPS.

A correção necessária é distinguir orçamento energético do perfil, integridade/exposição
do teste e desempenho/consumo em carga comparável ao stock. Simplesmente elevar198 para200W
não recupera dados de backends que não rodaram e não qualifica os12 pares rejeitados.

Referências: `crates/service/src/gpu_power_sweep.rs:5855`, `:5866`, `:6071`.

## 3. O domínio de clocks foi fechado antes de conhecer o máximo final

Cmax de descoberta foi1890MHz; a busca terminou no último bin acima de90%,1710MHz.
Quando o máximo publicável caiu para1710, o algoritmo não reabriu a busca inferior.
Porém, os pisos de seleção são calculados sobre o Godforge final:95% para Brokkr's e90%
para Deep Calm. Essas regiões se estendem abaixo dos1710MHz já explorados.

Por exemplo,90% de1710 é1539MHz, cujo próximo bin numa grade15MHz seria1545MHz.
Isso ilustra a lacuna de domínio, não uma recomendação de clock ou prova de que um ponto
mais baixo será eficiente. A etapa marcou `frontier_complete=true` porque completou o
intervalo inicial; não porque demonstrou opções econômicas relativas ao máximo final.

Referências: `crates/service/src/gpu_power_sweep.rs:9320`, `:10162`, `:6237`.

## 4. A publicação confunde um ponto qualificado com três objetivos atendidos

O seletor permite repetir Godforge quando não existem alternativas para os outros objetivos.
Com apenas1710 elegível, os três resultados são idênticos. A igualdade, isoladamente, poderia
ser legítima numa busca suficiente; aqui ocorreu após censura e esgotamento do intervalo.
Há aviso no log, mas a nota final apresenta os três como prontos, sem destacar essa limitação.

Além disso, o snapshot em disco limita o log às últimas40 linhas (o estado vivo mantém240). Os avisos
iniciais do cone e motivos de parada por clock deixam de estar no relatório final. As
observações preservam os dwells executados, mas não os candidatos censurados antes do dwell.

Referências: `crates/service/src/gpu_power_sweep.rs:2142`, `:6215`, `:11968`.

## Ordem de correção proposta

1. Persistir por clock a razão da parada e se a fronteira é medida ou censurada; comunicar
   quando Clean conserva restrições. Auditar a origem do incidente1920@943 e a amplitude
   inferida do cone. **Atualização posterior do usuário:** Full Reset deve apagar também as
   falhas aprendidas e os cones; Soft Reset as preserva. A política antiga desta análise foi
   substituída pela implementação documentada em `docs/contracts/ui-backend.md`.
2. Alinhar os objetivos energéticos e a comparação com stock, separando qualificação física
   de ranking do perfil. Fazer triagem energética na carga pertinente antes de investir toda
   a etapa longa; uma triagem curta não substitui a qualificação completa.
3. Após obter o máximo final elegível, completar o intervalo econômico que faltou com uma
   extensão limitada e explícita. Não reiniciar uma busca ilimitada nem perseguir1800@875.
4. Publicar claramente um único ponto qualificado quando não houver alternativas demonstradas;
   manter separadas qualificação do ponto e conclusão da busca dos três objetivos.

Esta análise não mudou algoritmo, limites, histórico ou serviço e não iniciou carga.
Snapshots, auditoria, resumo e hashes estão em `target/beta/profile-selection-20260917/`.
A run avançou até a qualificação de um par, mas ainda não valida a qualidade da descoberta
automática nem estabelece os melhores perfis desta GPU.

## Correção implementada em18/09 — validação física ainda pendente

- Full Reset já esquece positivos e negativos; os motivos por clock e pisos censurados agora
  também ficam estruturados no checkpoint/exportação, independentemente do log de40linhas.
- A qualificação não reprova mais apenas por atingir o teto energético comum de198W. Os quatro
  testes, exposição/controle de clock, integridade e limpeza continuam obrigatórios. Ranking usa
  a mesma carga PowerRender de calibração/stock; p99 e pico do estresse seguem visíveis à parte.
- Uma extensão econômica por run pode completar os bins até90% do Godforge final sustentado.
  O orçamento persiste em Resume. Se a faixa necessária mudar novamente, a limitação aparece.
- Pares idênticos são agrupados na UI, com qualificação e cobertura econômica separadas.
- Releitura offline:9 DX11 passes prosseguiriam para os backends que faltaram;3 pares continuam
  sem exposição suficiente. Isso não qualifica os pares antigos nem prevê quais perfis vencerão.
  Evidências de software: `target/beta/profile-policy-20260918/`.
