# Descoberta e desqualificação de pontos — contrato de 25/09/2026

Implementação atual: Discovery 10, Frontier 33, Exact Apply 38, busca 9 (29/09). Este documento
substitui as regras de residência/energia e a ordem de busca de 24/09. A matriz de cargas continua 27.

## Margem de jogo e topo compensado (29/09)

O Godforge 1920@925 da busca 8 deu TDR no Overwatch. A calibração com o usuário: 1800@875 é estável
sempre, e 1815@875 crasha o Overwatch em menos de 30 min. A matriz aprovou 1830@856, então é ~6 bins
mais branda que jogos.
- **Margem de jogo:** perfil só com outro par aprovado ≥36 mV abaixo no mesmo clock.
- **Topo compensado:** se a menor tensão aprovada no topo + 36 mV passa da tensão de potência, o
  clock desce (~10 mV por bin).
  - O par de margem (tensão de potência − 36 mV) precisa passar; depois o par publicado recebe a
    própria matriz.
  - Se o par de margem falhar, desce um bin, no máximo duas vezes.
- **Degraus:** −5%/−10% do topo compensado, descendo 2 bins por admissão. O par de publicação acima
  da faixa testada ganha admissão própria.
- **Balanceado:** a menos de 2% de clock do escolhido, o de menor potência vence.
- **Falha de campo:** a restauração ressintetiza os perfis e remove só o par condenado.

## Escada de descida e bin térmico (28/09)

Decisões do usuário após a run f2-forge-1790617016985. Essa run qualificou 1920/1830/1740 todos a
937 mV e parou em 58 min sem perfil: 931 mV deu ClockDrop de um bin nos três clocks, quente.
- **Escada:**
  - O topo é achado como antes. Depois a busca desce a tensão no clock do topo até a primeira
    instabilidade.
  - O degrau seguinte (−5%, depois −10% do topo) começa na menor tensão aprovada no degrau
    anterior, um bin acima da falha, e desce de novo. Os degraus rodam um de cada vez.
  - Substitui os itens 4–5 da busca abaixo e o teste de margem único de 27/09. A regra de margem da
    publicação continua: publica-se no mínimo um bin acima da menor tensão aprovada no clock.
- **Fim de cada degrau:**
  - A primeira falha de integridade, TDR ou prova incompleta encerra só o degrau (`integrity_edge`,
    `tdr_edge`, `evidence_boundary`) e não conta no orçamento de 2 erros.
  - O primeiro par de um degrau inferior repete uma tensão já aprovada num clock maior. Se ele
    falhar, a evidência é inconsistente e a busca para (`dominated_pair_failed`).
- **Bin térmico:**
  - Quente, a GPU perde um bin de clock na tensão travada da âncora, e mais cedo quanto menor a
    tensão: 937 mV por volta de 78 °C; 931 mV já a 71 °C, em 1920, 1830 e 1740.
  - Clock ≥ alvo − 15 MHz conta como mantido em toda a matriz: descoberta, residência, exposição
    DX11 (inclusive a fase leve) e alvo quente do Endurance.
  - Dois bins abaixo continuam falhando. O rótulo exato já não é provado quando quente.
- **TDR no limite do degrau:**
  - A run pausa com o par condenado e o cone aplicado.
  - Reinicie o Windows, reconheça o incidente e use Retomar: a mesma run continua no próximo degrau.
  - Continuar no mesmo boot é proibido (22/07: depois de um TDR, até o 1800@875 estável falhou).
  - No último degrau, ou se o orçamento de crashes recusaria a retomada, a run publica os pares já
    provados.
- **Orçamento de crashes:** bloqueia só nova exploração (Start/Retomar). O cone de TDR continua
  valendo para Apply, restauração de perfis e publicação após crash.

## Calor, margem e aceite em jogo (27/09)

- Sustentação das fases pesadas aceita **um bin abaixo** do alvo (1905 para 1920): sob carga pesada,
  quente e perto do limite, o boost da GPU desce um bin na mesma tensão. Residência, exposição DX11
  e descoberta continuam exigindo o alvo exato (ou o power cap), então isso não promove clock.
- "Térmico" nas dwells F2 = slowdown térmico de **hardware**. O bit de software aparecia a 70 °C
  com clock mantido e não é mais motivo de recusa; o power cap vale mesmo com ele.
- Endurance exige o alvo exato a até 3 °C da temperatura máxima da lane (≥ 100 amostras), senão
  `thermal_target_coverage_low`. Na run de 27/09 o 1920 rodou em fases médias até 79 °C.
- Margem: só entra nos perfis o par que tem outra tensão menor, no mesmo clock, também aprovada
  na run. O topo passa por um teste único um bin abaixo **no fim da run**, na última admissão
  (reservada); se falhar, o topo continua qualificado mas não é publicado. Erro nesse teste não
  conta no orçamento de 2 erros, e esse orçamento não cancela o teste. TDR no teste: Soft Reset.
- DX11 (ExactApply 36): 6 fases — pesada, rajadas 75/50/25%, **leve contínua** (1 instância,
  referência stock própria) e pesada. A leve precisa de ≥ 30 s no alvo exato, provando o DX11 no
  par abaixo do limite de potência, como um jogo leve.
- Métricas por fase gravam células `clock_temp` [clock, °C, amostras, amostras com power cap].
- Aceite: aplicar o perfil e jogar com o Safe Loop ativo. TDR em jogo sobe um bin e reaplica; se
  repetir, volta ao stock e condena o par. Travamento/tela azul: a inicialização volta ao stock.

## Contrato de carga representativa (26/09, noite)

Decisão do usuário, revertendo a regra de pior carga de 25/09: o topo é o maior clock que a carga
representativa (PowerRender) sustenta no alvo com potência estritamente abaixo do limite da placa.
Cargas mais pesadas da matriz podem atingir o limite; aí a GPU cai para os pontos stock elásticos
abaixo da âncora (offset 0), e essa queda não reprova nem torna o par inconclusivo.

- Amostra "no limite de potência": bit SW power cap do NVML, sem bit de slowdown térmico. A potência
  amostrada não serve para decidir: na Ampere o `power_usage` do NVML é média de 1 s, e rajadas de
  100 ms cortadas pelo limitador leem 100–190 W (run 1790466472114: bit em 100% das amostras DX11,
  73 s recusados como fora do alvo). Abaixo do alvo e no limite = sustentada; térmico conta contra.
- Texture/DX12/Endurance: residência e sustentação pesada (95%) contam essas amostras. Fases pesadas
  com menos de 20 amostras (abertura/fechamento de ~390 ms da triagem de 30 s) são ignoradas, não
  reprovam; pelo menos uma fase pesada precisa ser avaliada. Antes disso toda triagem curta saía
  `heavy_phase_telemetry_low` (run 1790448315552 terminou assim em 1920@937).
- DX11: `power_limited_active_ms` separado de `target_active_ms`; as fases de duty 100% e a regra de
  exposição (30 s / 35%) usam alvo + limite. O relatório continua mostrando a exposição real no alvo.
- Integridade, contenção (máx ≤ alvo+15, tensão ≤ bin), cobertura de fases/checksums, térmica e
  limpeza não mudaram. Seleção de perfis usa a p99 do PowerRender < limite; a p99 da pior lane
  continua publicada.
- Busca 7: power-bound no PowerRender salta direto para o menor bin acima da tensão média em que o
  limitador estabilizou (a GPU estabilizou num clock menor; potência não cai com clock maior, então
  o alvo não cabe acima dessa tensão). Na run de 26/09 seriam 2 admissões em vez de 24. Erro de
  integridade nessa tensão desce um clock; não há sondagem de ponto médio.
- Tradeoff: em jogos mais pesados que o PowerRender o perfil pode operar no limite de potência e
  abaixo do clock nominal. Isso é aceito por esta decisão; a exposição DX11 no alvo pode ser menor
  que 30 s quando o restante do tempo foi limitado por potência.

## Faixa nominal de clock (26/09)

A run1790446614161 repetiu pico1920 para alvo1905, sem exceder a tensão1043 e sem erro físico.
O produto agora qualifica o perfil nominal com faixa [alvo, alvo+15MHz]. O pedido NVML continua
no alvo nominal; não se eleva a configuração em15MHz. A mesma faixa vale na descoberta,
Texture/Vulkan, DX11, DX12 e Endurance. Tensão máxima continua no bin solicitado e potência
continua estritamente abaixo do limite da placa. Excesso de clock maior que15MHz é recusado.
Os15MHz são uma margem explícita do contrato, não uma garantia universal de comportamento.

Exposição em alvo..alvo+15 conta para sustentar o perfil nominal; amostras abaixo do alvo não
contam para a exigência pesada. Picos não promovem o perfil para o clock superior. Cada candidato
mantém seu próprio teste completo e sua tensão. Falhas físicas dentro dessa faixa reprovam a
configuração nominal; somente falhas fora da faixa perdem atribuição exata. `max_clock_mhz`
salva o pico absoluto em todos os resultados; p95 continua separado. Prova atual sem pico, com
pico acima da faixa ou de versões antigas não libera Apply. Diagnósticos DX11 ainda contam
excursões acima do nominal, mas somente excesso acima da faixa recusa o controle.

## Ordem de descoberta

1. Full Reset + Clean começa com as medições stock e os bins físicos desta GPU, sem usar
   1800@875 ou qualquer conhecimento particular do operador. As três estruturas de região
   continuam no IPC, mas somente performance começa habilitada. O primeiro clock é o maior
   bin do domínio stock da curva normalizada após aquecimento, nunca o p5 sustentado sob
   carga pesada. A tensão inicial é o bin planejável mais próximo da tensão stock medida,
   respeitando os limites de offset. O topo da curva é hipótese, não prova de sustentação.
2. Cada candidato passa por PowerRender (10 s), triagem Texture (30 s no Standard) e matriz
   completa: DX11 7 min, Vulkan/Texture 2 min, DX12 2 min, Endurance 5 min. Uma triagem
   aprovada sozinha não aprova um perfil nem autoriza refinamento ordinário.
3. Limitação energética confirmada permite descer um bin de tensão no mesmo clock, gastando
   outra admissão. Se atingir o piso de tensão sem um ponto qualificado, tenta o próximo clock
   físico inferior na mesma tensão. Não existe repetição oculta ilimitada.
4. Sem topo qualificado, o primeiro erro de integridade permite investigar o próximo clock
   físico inferior. Se veio após descida por potência, retorna um bin de tensão para sair do
   par que falhou; isso não aprova o novo par. O teto desta busca desce junto, evitando voltar
   ao clock recusado. Dois erros encerram a busca e TDR interrompe imediatamente.
   Uma aprovação completa nesse topo encerra performance e libera as regiões econômicas.
   Inconclusividade fecha só a banda (26/09), sem inferir fronteira nem autorizar elevar
   tensão. Sem topo qualificado não há bandas econômicas, então a busca termina
   (`qualified_top_unavailable`); numa banda econômica a outra continua.
   A busca é finita: não prova que todos os pares acima do melhor encontrado são inviáveis.
5. Depois do fechamento dessa busca com um topo qualificado, equilíbrio e eficiência começam
   em candidatos próximos de 95% e 90% DESSE topo, arredondados aos bins físicos, e refinam
   tensão. Esses são pontos de partida, não os três resultados prometidos. A seleção compara
   todos os pares completos disponíveis na faixa de 90–100% do melhor topo qualificado.
6. Permanecem 24 admissões/8 h no Standard, com tempo maior no Long conforme sua matriz.
   Cada admissão é persistida e relida antes de aplicar. Cancelamento e reinício não devolvem
   tentativas. Dois erros de integridade encerram a busca; TDR/falha operacional encerra antes.
   Crash atribuído (26/09, decisão B): a run para, mas depois que a condenação fica durável e o
   stock é restaurado, os pares que já tinham matriz completa são publicados como perfis, excluindo
   o cone TDR recalculado do ledger. O Apply continua bloqueado pelo incidente pendente até o
   reconhecimento e revalida o cone no Apply. Bugcheck (processo morto) ainda não publica.
   O limite finito pode deixar regiões sem exploração: “melhor encontrado” não prova ótimo global.
7. Exceção limitada para excursão de controle na descoberta: com evidência neutra persistida,
   contrato atual, retorno stock e BootFlag limpo, permite uma reaplicação do MESMO par em
   toda a run. Consome nova admissão e o contador persiste no Resume. Outra excursão encerra
   com `control_reapplication_failed`; falhas de qualificação/Apply/reset/TDR não usam essa
   exceção. Nenhuma excursão aprova o par ou autoriza avanço de clock/tensão. O motivo salva
   teto solicitado e máximos amostrados para diagnosticar a causa, ainda não provada fisicamente.

## Algoritmos que recusam evidência ou desqualificam candidatos

| Verificação | Critério aplicado | Consequência |
|---|---|---|
| Aplicação e readback | Tensão travada no bin solicitado; readback da trava em até 1 s; curva ancorada e pedido de teto nominal devem ser aceitos/verificados; tolerância de readback da curva é 15 MHz, distinta da faixa runtime nominal..nominal+15 | Falha operacional; não executar uma matriz como se a aplicação tivesse ocorrido |
| Teto de clock | Máximo amostrado acima de alvo+15 MHz; usa máximo, não p95. O sampler acompanha clock independentemente da leitura de potência, incluindo rampas/transições | Falha de controle: descarta o teste sem inferir tensão insuficiente; somente descoberta com recuperação limpa pode usar a reaplicação única do item 7; demais casos encerram a busca |
| Autoridade de tensão | Pelo menos 3 leituras válidas; máximo observado não pode superar o bin solicitado (tolerância 0 mV). Rampas também entram no máximo | Sem autoridade: inconclusivo; acima do teto: falha de controle. Não autoriza descer tensão a partir de ClockDrop sem essa prova |
| Sustentação no DX11 pesado | Nas duas fases de duty 100%, cada uma precisa de pelo menos 30 s de tempo ativo amostrado e pelo menos 95% desse tempo na faixa alvo..alvo+15 ou no limite de potência, com tensão válida | `heavy_clock_not_sustained`; visitas ao alvo nas fases leves não compensam falha nas pesadas |
| Exposição DX11 variável | 5 fases concluídas; pelo menos 60 s ativos observados nas fases variáveis, 30 s no alvo ou no limite de potência e fração de pelo menos 35%; nenhuma excursão superior | Telemetria/exposição insuficiente: inconclusivo; não é blacklist |
| Sustentação nas outras cargas | Fases pesadas com pelo menos 20 amostras precisam de 95% na faixa alvo..alvo+15 ou no limite de potência; fases mais curtas são ignoradas, mas pelo menos uma precisa ser avaliada; PowerRender de descoberta exige p5 pelo menos no alvo e máximo dentro da faixa | Recusa por falta de sustentação (ou `heavy_phase_telemetry_low`). ClockDrop atual não vira, sozinho, fronteira de instabilidade |
| Potência na carga representativa (descoberta PowerRender) | Pico medido em precisão float precisa ser estritamente menor que o limite real da placa; atingir 200 W já recusa numa placa limitada a 200 W. Lanes da matriz podem atingir o limite | Power-bound: salto para o menor bin acima da tensão de equilíbrio medida no mesmo clock |
| Contraste/cobertura da carga | Fases e checksums obrigatórios; pelo menos 20 amostras BoostEdge na Texture/Endurance; contraste pesado–leve de pelo menos 3 W quando mensurável; residência global mínima 35% | A carga não provou o ponto; inconclusivo. Foi removida a exceção de reduzir residência para 5% sob power limit |
| Integridade | Erro de checksum contra golden stock ou resultado de instabilidade do motor; secundário também usa golden stock e cancela o parceiro na falha | Reprovação física; encerra região ou faz o reparo limitado descrito acima, respeitando o orçamento de dois erros |
| Telemetria travada | Ausência de amostra válida por pelo menos 300 ms após leituras válidas | `telemetry_stall`: inconclusivo, não erro de cálculo nem blacklist automática |
| Temperatura | Na descoberta, throttling térmico recusa calibração. Na qualificação, avalia queda nas fases pesadas, sem usar p5 de idle para condenar o ponto | Inconclusivo térmico; não reparar com tensão maior |
| Potência inconsistente | Variação de p99 entre medições comparáveis maior que max(8 W, 5%), com diferença de p5 ≤15 MHz | Medição não confirmada; nenhuma retentativa fora do orçamento. A faixa 98–99% do limite não exige repetição apenas pela antiga histerese |
| Cancelamento | Stop sem erro físico concomitante | Cancelado; candidato não vira blacklist nem fronteira. Erros físicos simultâneos continuam registrados |
| Recuperação | DeviceLost/TDR, reset stock não confirmado, BootFlag não limpo, falha de persistência/readback | Interromper e exigir recuperação; nenhum perfil liberado com transação incompleta |
| Publicação | Mesma GPU, run, par e versões atuais, matriz ordenada completa, limpeza confirmada, potência válida | Provas antigas, par diferente e aprovação apenas na triagem não liberam Apply |

## Precisão e limites

“95% sustentado” é um critério explícito de medição, não promessa de clock contínuo e imutável.
Downclock em idle, checksums de CPU e baixa demanda não reprova por si só. No DX11, cada leitura
precisa caber inteiramente num intervalo de trabalho; o crédito temporal é limitado a ±15 ms e
cortado pelos intervalos e amostras vizinhas. Não se inventa exposição em lacunas do sensor.

Quando a medição registra uma falha física junto com excursão fora do par, conserva o resultado
bruto e a recuperação, mas não grava blacklist nominal pelo motor de dwell nem aprende aquela
falha como fronteira de tensão do par. A proteção independente de TDR/reboot continua conservadora:
um incidente já atribuído pelo Sentinel/BootFlag não é apagado automaticamente por esta revisão.
O relatório de incidente deve ser analisado separadamente da prova de estabilidade do par.

O programa solicita teto pela curva e NVML; isso não é garantia de que o driver jamais ultrapassará
esse teto. Ele passa a recusar a evidência quando observa a violação. Amostragem de aproximadamente
30 ms não detecta necessariamente todos os transientes, e nenhuma matriz sintética finita garante
estabilidade em todos os jogos. Os motores existentes foram mantidos; estas mudanças corrigem
controle da evidência, cobertura e busca, sem alegar que um novo shader foi fisicamente validado.

## Arquivos e validação

- `crates/service/src/qualified_search.rs`: admissão, prioridades, transições e limites.
- `crates/service/src/dx11_residency.rs`: exposição ativa e prova das fases pesadas.
- `crates/service/src/gpu_power_sweep.rs`: medições, cobertura por fase, orquestração e seleção.
- `crates/service/src/gpu_undervolt.rs`: classificação, autoridade, transação e recuperação.
- `crates/core/src/f2_observation.rs`: versões, prova persistida e interpretação do aprendizado.
- Evidências de software e backup anterior: `target/beta/worst-load-20260925/`.

A próxima aceitação física deve usar Full Reset → Clean pelo BAT do usuário, após fechar o Core
anterior. O BAT recompila a versão release; reinstalação não é necessária. Examinar primeiro os
motivos concretos e as fases pesadas, sem ajustar limites só para aprovar a referência manual.

Verificação de software: 690 testes Rust passaram (3 de hardware ignorados); após a última
correção do classificador, os 476 testes do serviço passaram novamente.18 testes unitários da UI
e6 testes Playwright de qualificação passaram. Builds release/Core e produção/UI verificados.
As evidências e hashes finais estão em `target/beta/worst-load-20260925/verification.json`.
