# Auditoria F2: precisão de desqualificação e saída do ciclo de testes

Data: 2026-08-04  
Escopo: repositório completo, caminho NVIDIA/F2 ativo, aplicação de curva, Safe Loop, Detector Lab,
persistência, contratos e ensaios reais na RTX 3060 Ti deste computador.

## Veredito

**GO para a qualificação sintética limitada v27 e sua contenção de falha; NO GO para alegar que um
único teste finito desqualifica todo ponto instável com precisão determinística.**

Atualização pós-calibração: o operador preservou o objetivo central de qualificação sintética em um
clique e aprovou cobertura por API apenas nos pares finais deduplicados. A matriz v27 congela os
workloads existentes, move o DX11 v2 residente para o início e acrescenta contenção de TDR; ela não
abre outra sequência de shaders/challengers. A busca por um detector perfeito termina aqui.

## Fechamento de arquitetura em 2026-08-25 — software pronto, aceitação física adiada

O veredito metodológico acima não mudou: nenhum teste sintético finito virou prova universal de
estabilidade. O que mudou foi a capacidade do produto de tomar uma decisão **finita, reproduzível e
fail-closed** sem reabrir o ciclo de criar workloads indefinidamente:

- Discovery7 mede a descida; Frontier28 filtra cada fronteira; somente pares finais deduplicados pagam
  ExactApply29/matrix27 (DX11 v2 → Vulkan → DX12 → Endurance). Inconclusive bloqueia publicação, não é
  convertido em pass/fail.
- Os TDRs reais v29 em `1920@931` e `1860@900` alimentam um cone físico 1 bin de clock : 1 bin de
  tensão. Bins no cone são censurados antes de write; mais de dois CandidateCrash efetivos encerram
  novas campanhas de busca. Não existe repeat-until-fail.
- Quarantine deixa o par exato retestável, mas exige duas matrizes completas ordenadas no mesmo
  run/GPU/par/contrato. Rigid recusa o par e tudo abaixo. Publicação continua monotônica e só usa
  pares realmente medidos.
- A auditoria adversarial final encontrou quatro bypasses operacionais e todos foram fechados:
  CandidateCrash agora precisa de flush + readback no ledger; Sentinel precisa comprovar watcher
  pronto antes de reapply; Benchmark F2 usa o writer proof-aware; e workers legados exigem BootFlag
  owned com estados negativos legíveis. Full Reset remove apenas positivos e preserva todo negativo.

Validação offline final: 674 testes passaram, dois smokes de hardware ficaram explicitamente
ignorados, e workspace check, UI production build, Clippy e diff-check passaram. Por decisão do
operador, nesta sessão não foram iniciados serviço, ACK, run ou carga de GPU. Assim, o programa está
**pronto para a próxima run monitorada**, mas os três perfis ainda não estão fisicamente aceitos por
esta revisão; essa evidência fica deliberadamente para outro dia.

## Revalidação real em 2026-08-10 — driver 610.62

A matriz v27 foi reexecutada no hardware após a atualização do driver. O resultado não justifica
novo workload nem relaxamento do gate; ele demonstra por que a margem e a residência exata são
necessárias:

| Ponto aplicado | Resultado | Entrega física | Decisão |
|---|---|---|---|
| `1800@875` | `Inconclusive` em DX11, 420 s | 1785 MHz p5/avg/p95, 875 mV, 119.424 frames, 14.928 checks | bloqueado por `target_residency_low`; matriz encerrou antes das outras APIs |
| `1800@887` | `Stable`, DX11 60 s | 1800 MHz exatos, 17.744 frames, 1.109 checks | prova curta de entrega após +12 mV |
| `1815@887` | `Stable`, DX11 60 s | 1815 MHz exatos, 17.808 frames, 1.113 checks | candidato para a matriz completa |
| `1815@887` | `Stable`, matriz v27 completa | 1815 MHz e 887 mV em 100% das amostras válidas | candidato limitado aprovado |

Na matriz completa, DX11 v2 produziu 120.608 frames/15.076 checks em 420 s; Vulkan,
7.249/933 em 120 s; DX12, 7.286/980 em 120 s; e Endurance, 57.227/5.445 em 300 s.
Foram 192.370 frames e 22.434 checks no total. As potências médias por lane foram 110,05 W,
143,46 W, 146,41 W e 146,89 W; Endurance atingiu p99 de 177,12 W e máximo de 69 °C. Não houve
evento `nvlddmkm` no boot, boot flag, Safe Mode, crash streak ou recuperação pendente, e o cleanup
confirmou stock.

Conclusão adicional: `1800@875` continua sendo um controle eletricamente estável, mas não pode ser
tratado como controle de **entrega exata** no driver 610.62. Relaxar a residência para aprová-lo
fabricaria evidência para 1800 MHz quando o workload só exerceu 1785 MHz. O fato de `1815@887`
sustentar o alvo confirma empiricamente a política existente de `fronteira + 12 mV + snap físico`.
O driver/build deve permanecer na proveniência; uma verdade observada no 595.97 não é automaticamente
transportável para o 610.62.

Os journals brutos são `C:\ProgramData\Nidavellir\detector-lab-1786388939075.jsonl` e
`C:\ProgramData\Nidavellir\detector-lab-1786389954809.jsonl`. Uma tentativa única de relançar
`1815@875` foi barrada pelo UAC antes de criar journal ou aplicar o ponto; portanto não há resultado
novo desse controle e nada foi inferido dele.

Durante a campanha, Detector Lab expôs uma falha de observabilidade: o percentual permanecia
congelado dentro das lanes e o preflight stock não identificava a API ativa. A correção atualiza
progresso e fase, sem alterar algoritmo, ordem, duração, margem ou classificação.

O problema principal não é falta de mais uma fase sintética. A investigação mostrou três fatos:

1. o ponto escolhido no MSI Afterburner é uma configuração de curva, não prova de que a GPU ficou
   continuamente naquele par clock-tensão;
2. o modo v25 com voltage lock e o modo de curva flat são experimentos fisicamente diferentes;
3. o workload atual deixou passar, por 300 s, o controle historicamente mais agressivo disponível
   (`1860@868` em modo de curva), mas o Overwatch 2 derrubou exatamente esse ponto, sem stretch de
   clock ou escape de tensão, após 83,8 s de carga alta contínua no trecho final.

Portanto, `1815@875` não pode continuar rotulado como “instável exato”. Ele é atualmente
**indeterminado**: os testes manuais antigos podem ter exercitado outro bin efetivo, enquanto os
ensaios atuais não observaram falha. O mesmo vale para `1800@869`, resolvido fisicamente para
`1800@868` pelo driver.

O detector v25 continua útil como barreira de segurança e cobertura, mas sua sensibilidade para os
defeitos reais desta GPU foi refutada por um falso negativo confirmado. A falha independente agora
existe e revela uma diferença concreta: o jogo usou DirectX 11 com uma carga e um modelo de filas que
o qualificador principal Vulkan/wgpu e o antigo probe DX11 offscreen não reproduzem.

## O que foi auditado

- Documentos de continuidade, arquitetura, decisões, roadmap e contrato UI/backend.
- Grafo indexado: aplicação, descoberta, qualificação, publicação, recuperação, Detector Lab e
  Sentinel.
- Hotspots estruturais e caminhos de classificação nos crates Rust.
- Journals e observações reais em `C:\ProgramData\Nidavellir`.
- Histórico do repositório para reconstruir a receita que produziu o erro de julho.
- Estado de boot, Safe Loop, boot flag, serviço, telemetria e eventos Windows.
- Campanha ao vivo com aplicação exata, bins corrigidos e aplicação experimental de curva.

Não foi feita limpeza cosmética em massa. Nos motores de hardware, refatorar amplamente enquanto o
estimando físico ainda está sendo corrigido misturaria alteração estrutural e alteração experimental.
As mudanças são cirúrgicas e rastreáveis aos defeitos observados.

## Estado atual do sistema

O caminho ativo é:

`stock → descoberta → qualificação → fronteira publicável → exact Apply → perfis`

Há três conceitos que precisam permanecer separados:

| Conceito | Pergunta respondida |
|---|---|
| resultado do workload | Houve erro silencioso, instabilidade, crash ou TDR? |
| fidelidade da aplicação | Quais clocks e tensões foram realmente exercitados? |
| publicabilidade | A evidência satisfaz o contrato atual para desbloquear Apply? |

O Detector Lab é diagnóstico e não publicável. Ele não deve escrever blacklist, qualificar perfil
ou desbloquear Apply.

### Pontos fortes

- Safe Loop é armado antes das mutações e a recuperação diagnóstica não contamina blacklist/crash
  streak.
- `Fail` físico e `Inconclusive` de cobertura/telemetria são separados.
- Contratos e fingerprints são versionados; evidência positiva antiga não desbloqueia o contrato
  atual.
- O modo v25 bloqueado verifica tensão e impõe teto máximo de clock.
- O controle stock completo antecede o candidato em `control_v25` e `curve_v25`.
- O journal agora registra o início de cada segmento antes do trabalho de GPU.

### Dívida estrutural relevante

| Área | Evidência do grafo | Risco |
|---|---:|---|
| `measure_multiclock_undervolt_forge` | 1.993 linhas, ciclomática 149, cognitiva 549 | Estado, política e I/O muito acoplados |
| `run_confirmed_f2_clock_discovery` | 1.224 linhas, ciclomática 114, cognitiva 362, 19 parâmetros | Invariantes e retomada difíceis de provar |
| `load_and_measure_for` | 292 linhas, ciclomática 38, cognitiva 85 | Render, sampler e classificação compartilham o limite de falha |
| `qualification_coverage_from_run` | 185 linhas, ciclomática 23 | Muitas regras de autoridade concentradas |
| `handle_request` | 420 linhas, ciclomática 122 | Orquestração IPC extensa, embora não cause o falso negativo |

Esses hotspots justificam refatoração futura por extrações puras e testes de equivalência. Eles não
justificam reescrita durante a calibração física.

## Descoberta principal: par exato e curva flat não são o mesmo ensaio

O relato do operador esclareceu que os rótulos históricos vieram do MSI Afterburner, usando a curva
clássica achatada após o ponto selecionado. Esse ajuste não garante que “1815@875” tenha sido
continuamente 1815 MHz a 875 mV. O driver pode deslocar bins, reduzir clock por regime e selecionar
outros pontos da envoltória.

O Nidavellir v25 bloqueado faz algo mais rígido: curva ancorada, teto de clock e voltage lock com
readback. Esse modo é útil para isolar o par, mas suprime parte das transições VF existentes no uso
do Afterburner. Comparar diretamente um fracasso manual de curva com um passe bloqueado é comparar
estimandos diferentes.

Foi criado `curve_v25` apenas para diagnóstico. Ele usa a mesma curva ancorada e o mesmo teto de
clock, mas não aplica voltage lock. Não é declarado byte-idêntico ao Afterburner; é a aproximação
controlada disponível para observar a envoltória dinâmica. Ele nunca publica evidência e sempre
retorna a GPU a stock ao terminar.

## Campanha ao vivo

Ambiente observado: NVIDIA GeForce RTX 3060 Ti, driver 610.62, limite de potência de 200 W. Não houve
TDR, bugcheck, blacklist, crash streak ou boot flag residual na campanha sintética descrita abaixo.
O ensaio de campo posterior com Overwatch teve TDR e bugcheck e está documentado separadamente.

### Ensaios de 60 s com voltage lock

Foram executados 13 candidatos, além dos controles stock exigidos pelas receitas:

| Configuração | Receita(s) | Repetições | Resultado |
|---|---|---:|---|
| `1800@875` | `control_v25`, `dense_v14` | 3 | 3 estáveis |
| `1815@875` | `control_v25`, `dense_v14` | 4 | 4 estáveis |
| `1800@868` | `control_v25`, `dense_v14` | 2 | 2 estáveis |
| `1830@875` | `control_v25` | 2 | 2 estáveis |
| `1815@868` | `control_v25` | 2 | 2 estáveis |

`1830@875` e `1815@868` foram incluídos para testar a hipótese de deslocamento de um bin nos
rótulos antigos. Nenhum foi rejeitado em 60 s. Em uma repetição de `1815@868`, o p5 caiu para
1800 MHz e a residência no alvo foi 91,35%; isto confirma que até com tensão bloqueada o rótulo de
clock representa teto/alvo com residência medida, não clock contínuo em todas as fases.

Conclusão limitada: o detector atual não reproduziu os fracassos manuais em 60 s. Isso não prova que
os pontos sejam estáveis no uso real; prova que os rótulos antigos não podem servir como ground truth
exato e que o detector não os discrimina nesta duração.

### Controle de curva `1815@875`

`curve_v25`, 60 s:

- workload e cobertura: `stable`/`Pass`;
- clock p5/p95: 1815/1815 MHz;
- tensão min/média/máx: 875/875/875 mV;
- residência no alvo: 100%;
- 3.485 frames e 442 checksums.

Nesta carga específica, mesmo sem voltage lock, o driver escolheu o anchor continuamente. Ainda
assim, uma repetição limpa não valida estabilidade de longo prazo.

### Controle historicamente ruim `1860@868`

O ledger histórico contém múltiplas reprovações independentes do perfil nominal `1860@868`,
incluindo erro silencioso em Endurance e em Texture. O registro bruto de 2026-07-16 falhou em
Texture/ROP depois de aproximadamente 253,9 s. Como a telemetria antiga também escapava de tensão,
esse é um controle positivo de **curva/envoltória**, não de par exato.

Foi executado `curve_v25` por 300 s:

- todas as fases do workload: `stable`;
- cobertura: `Pass`;
- 17.971 frames e 2.372 checksums;
- clock: média 1858, p5 1845, p95 1860 MHz;
- residência global no alvo: 89,01%; na fase Mixed Game: 0%;
- tensão: mínima 868, média 874, máxima 1037 mV;
- potência: média 145,25 W, máxima 178,88 W;
- temperatura máxima por fase: até 74 °C;
- nenhuma falha ao atravessar a segunda janela Texture e o tempo histórico de 253,9 s;
- nenhum TDR/evento Windows correlacionado.

O build usado no ensaio classificou o resultado como `inconclusive` apenas porque aplicava a regra
`max_voltage <= anchor` também ao modo de curva. Essa regra é correta no modo bloqueado e incorreta
no modo elástico. Reclassificando os mesmos dados com as dimensões separadas, o resultado é:

- **workload:** `stable` — nenhum detector encontrou o defeito;
- **fidelidade:** `anchor_voltage_escaped=true`, máximo 1037 mV;
- **publicabilidade:** `false`, por definição do laboratório.

Isso é o resultado decisivo da auditoria: o conhecido ruim histórico passou no workload atual por
300 s. Aumentar o tempo sem uma hipótese nova não converte esse workload em detector preciso.

### Ground truth atual: Overwatch 2 em `1860@868`

Às 18:05:38, foi aplicada uma curva ancorada com teto de 1860 MHz no bin físico de 868 mV, sem
voltage lock, e iniciado o Game Trace a 10 ms. O jogo iniciou o render às 18:06:39 e o primeiro bloco
de partida começou aproximadamente aos 97,9 s do traço. O primeiro TDR ocorreu às 18:11:21.

| Evidência | Resultado |
|---|---:|
| amostras válidas | 32.525 |
| duração total do traço | 344,2 s |
| clock observado | 1860/1860 MHz, mínimo/máximo |
| tensão observada | 868/868 mV, mínimo/máximo |
| residência exata em `1860@868` | 100% |
| tempo acumulado com utilização >= 30% | 173,3 s |
| tempo acumulado com utilização >= 80% | 149,8 s |
| tempo acumulado com utilização >= 95% | 133,3 s |
| último bloco contínuo de carga alta | 83,8 s |
| últimos 60 s antes do TDR | 98,83% de uso; 155,37 W médios; 163,17 W p95; 70 °C máx. |
| último canário | terminou 11,5 s antes do TDR; resultado estável |

O relato sobre stretch do Afterburner não explica esta falha: o traço não observou 1875 ou
1890 MHz, nem tensão acima de 868 mV. Para este ensaio, `1860@868` é um positivo físico confirmado.

O primeiro evento `nvlddmkm 153` foi seguido por mais sete eventos 153 em 15 s, um evento
`nvlddmkm 14`, desligamento inesperado e bugcheck `0x116`. O WER apontou `nvlddmkm.sys`; o log do
jogo registrou `Executing Lost Device Callback` às 18:11:27. Não foi um falso alarme de telemetria:
foi uma cascata TDR que terminou em reinicialização do Windows.

O monitor identificou o primeiro evento em aproximadamente 1,06 s e fechou o arquivo de traço, mas
o retorno a stock não concluiu antes da cascata. Após o reboot, a GPU voltou fisicamente a stock,
porém o `boot_flag.json` continuou armado em `game_trace_curve_diagnostic`. A Safe Loop permaneceu
idle, sem blacklist ou crash streak, como exigido para um diagnóstico não-learning. A ausência de
evento terminal do Sentinel só permite afirmar que o reset não concluiu; como o journal é escrito
depois das chamadas NVML/NVAPI de reset, hoje não é possível distinguir atraso do watcher de bloqueio
da chamada de driver.

### Diferença concreta entre o jogo e os qualificadores

O log local do Overwatch registra:

- API gráfica `Dx11`;
- uma fila graphics, uma compute e uma copy;
- janela 2544x1353, cap de 600 FPS, Reflex ativo e `CpuForceSyncEnabled=1`;
- preset gráfico 4, texturas 3, reflexos locais, SSAO e SSR ativos.

O qualificador principal usa Vulkan/wgpu. O caminho DX11 já existente também não representa o
jogo: ele renderiza offscreen em 768x768, executa apenas um pixel shader ALU, usa um único immediate
context, chama `Flush` e espera a GPU após cada frame, e faz readback periódico. Não há swapchain ou
`Present`, textura, depth/ROP realista, compute/UAV concorrente, streaming/copy ou fila de frames.
Nos seis registros históricos coletados, esse probe consumiu apenas 99–133 W e nunca rejeitou um
candidato; por isso foi removido do gate no contrato v19.

Também não basta igualar escalares. Versões anteriores mantiveram `1860@868` por cerca de 303 s,
chegaram a 181 W e passaram. O v25 manteve 150 s de Field Concurrency em `1860@868`, mas com 144 W
médios e quedas cíclicas. O Overwatch falhou com potência menor que alguns sintéticos. O discriminante
faltante é o grafo de trabalho e o agendamento DX11, não simplesmente duração, calor ou watts.

## Por que ainda não existe desqualificação precisa

### 1. A verdade histórica descrevia settings, não estados físicos

`1800@875`, `1815@875` e `1800@869` eram posições selecionadas numa curva do Afterburner. Sem a
distribuição observada de clock e tensão durante a falha, não sabemos qual bin físico falhou.

### 2. O ground truth independente agora existe, mas não deve ser repetido cegamente

O traço atual, o TDR, o bugcheck e o histórico manual concordam sobre `1860@868`. Uma repetição seria
útil para estimar taxa, mas provocar novos bugchecks apenas para obter 2/3 não é proporcional ao
risco. Uma falha física exata já é suficiente para provar o falso negativo do detector atual.

### 3. O controle positivo mais forte produziu um falso negativo confirmado

`1860@868` em modo de curva cruzou 300 s sintéticos sem erro e depois derrubou o Overwatch no mesmo
par físico. Logo não há base para afirmar que variações pequenas de fases/ordem Vulkan aumentarão
precisão.

### 4. Aplicação, cobertura e falha estavam parcialmente colapsadas

Uma tensão acima do anchor em curva elástica é evidência sobre a aplicação, não erro do workload.
Misturar isso em `inconclusive_reason` escondia se o detector realmente encontrou algo. O journal
agora registra `workload_result`, `application_mode`, `anchor_voltage_escaped` e as tensões
observadas separadamente.

### 5. Um passe e uma falha não medem taxa

Sem repetições alternadas, ordem, estado térmico, churn do driver e aleatoriedade do silício podem
ser confundidos com capacidade do detector. Também é necessário medir inconclusivos e sessões sem
fechamento, não apenas pass/fail.

### 6. Mais tempo, calor ou potência não garantem discriminação

O ensaio de 300 s e qualificadores antigos mais quentes já excederam a janela real. Dwell maior
aumenta risco operacional, mas não corrige uma carga na API e no grafo de comandos errados.

### 7. O worker de GPU ainda não tem isolamento de processo

`load_and_measure_for` chama a carga sincronamente. Uma chamada de driver que nunca retorna não pode
ser encerrada com segurança por outra thread Rust. O journal fino melhora atribuição, mas timeout
real exige processo worker isolado e política explícita de reboot.

### 8. O canário não amostra o modo de falha observado

Nove canários TextureRop de 1,14–1,52 s terminaram estáveis durante a sessão. O último acabou 11,5 s
antes do TDR e não estava ativo no momento da falha. Portanto, esse canário é um falso negativo para
esta classe e não deve ser usado como argumento de estabilidade ou mecanismo confiável de prevenção.

### 9. Recuperação depois do primeiro TDR é uma rede final, não prevenção

O Event Log permitiu detectar o primeiro TDR, mas o driver já entrou em cascata. Uma chamada
NVML/NVAPI após o evento pode atrasar ou bloquear, e o journal atual só é escrito depois dela. O
qualificador deve procurar corrupção, timeout ou degradação antes do TDR; não pode depender de reset
userspace posterior para evitar reboot.

## Mudanças implementadas

- Journaling de `candidate_recipe_start` e `segment_start` antes de cada segmento.
- Recuperação específica de `detector_lab`, sem blacklist, crash streak ou Safe Mode falsos.
- Boot flag retido até a limpeza real dos controles de hardware.
- Runner elevado `scripts/run-detector-trial.ps1` com preflight, telemetria periódica, fechamento,
  reset cooperativo e parada no primeiro TDR.
- Orquestrador finito `scripts/run-detector-campaign.ps1`.
- Aplicação diagnóstica `curve_v25`, sem voltage lock, não publicável e com reset final obrigatório.
- Classificação separada entre resultado do workload e fidelidade ao anchor. Escape de tensão em
  `curve_v25` deixa de virar falso `Inconclusive`; ausência/baixa telemetria continua inconclusiva.
- O journal terminal passa a registrar `workload_result`, `application_mode`,
  `anchor_voltage_escaped`, `voltage.anchor` e `voltage.ceiling` apenas quando autoritativo.
- Aplicação diagnóstica de curva + Game Trace a 10 ms, com correlação de canário e TDR, usada para
  capturar a primeira falha de campo com par físico exato.

Não houve alteração de workload de produção, threshold de qualificação, fingerprint, duração do
Forge, schema IPC ou regra de publicação. O modo de curva permanece diagnóstico e não foi exposto
como receita de produto.

## Validação de software e estado final

- `cargo test --workspace`: 587 testes passaram; um smoke DX11 de hardware permaneceu ignorado.
- `cargo check --workspace`: passou.
- `npm.cmd run build`: build de produção da UI passou.
- `cargo clippy --workspace --all-targets`: passou, com warnings preexistentes fora do escopo.
- Sintaxe dos dois runners PowerShell: válida.
- `git diff --check`: passou; a baseline global de rustfmt continua divergente e não foi reformatada
  em massa.
- Após a campanha sintética: GPU em stock, Safe Loop idle, ponto manual inativo e nenhum TDR.
- Após o ensaio Overwatch: reboot às 18:12:51 deixou a GPU fisicamente em stock, serviço parado,
  Safe Loop idle e sem aprendizado diagnóstico; o boot flag residual continua armado e deve ser
  reconciliado antes de outra mutação.

## Protocolo anterior (superseded pela decisão v26 abaixo)

### Gate 1 — ground truth encerrado

`1860@868` é o positivo rígido desta GPU. Não repetir Overwatch nesse ponto apenas para elevar a
contagem estatística. Preservar o trace, log do jogo, eventos Windows e boot flag como um único
episódio correlacionado.

### Gate 2 — um único challenger DX11 v2

Reutilizar o backend DX11 existente, mas substituir o microbenchmark serial por uma carga com:

- API nativa DX11 e swapchain/`Present`;
- múltiplos frames em voo, sem `Flush` + espera a cada frame;
- textura, depth/ROP, pixel ALU, compute/UAV e streaming/copy no mesmo ciclo;
- checksum periódico e timeout em processo worker isolado;
- curva elástica, não apenas voltage lock;
- janela de medição que sustente 98–100% de uso e a faixa observada de aproximadamente
  152–163 W por pelo menos 90 s após aquecimento.

Isso é uma hipótese única: **API/agendamento DX11 realista**. Não criar variantes de shaders ou
ordens durante a matriz.

### Gate 3 — matriz curta e regra de saída

- stock: duas execuções de 120 s;
- controle seguro `1800@875` em curva: duas execuções de 120 s;
- positivo `1860@868` em curva: uma execução de até 120 s, encerrada na primeira falha;
- ordem fixa registrada antes do primeiro ensaio; reboot obrigatório após qualquer TDR;
- zero rejeições ou journals abertos nos quatro controles;
- promoção experimental somente se o positivo for rejeitado e os quatro controles forem limpos.

Se o DX11 v2 também deixar o positivo passar, encerrar o desenvolvimento de qualificadores
sintéticos para esta etapa. O produto deve parar de prometer qualificação precisa e usar uma fronteira
conservadora conhecida (`1800@875` nesta GPU) mais probation em jogo real para perfis novos. Esse é o
fallback que impede o ciclo infinito.

## Regras de parada

1. Não repetir o positivo real apenas para aumentar a amostra.
2. Máximo de um challenger: DX11 v2.
3. Hipótese, métricas, duração e ordem registradas antes da coleta.
4. Nenhuma receita ou shader novo no meio da matriz.
5. `Inconclusive` não vira falha.
6. Escape do anchor em modo de curva não vira falha de workload.
7. Se DX11 v2 não rejeitar o positivo, abandonar a alegação de qualificação sintética precisa.
8. Após promoção ou rejeição, documentar e parar.

## Conclusão da auditoria anterior

O relato do Afterburner resolveu a contradição central: os rótulos antigos não eram pares físicos
absolutos. `1800@875` continua sendo uma verdade operacional forte para esta GPU, mas `1815@875` e
`1800@869` não são negativos exatos confiáveis. Os testes atuais permitem que `1815@875` seja válido;
eles não provam isso para uso prolongado.

O estado honesto agora é mais forte: **`1860@868` falhou fisicamente sem stretch e o workload atual
produziu um falso negativo. O fator que escapava é o caminho DX11 e seu agendamento, não mais tempo ou
mais watts. A saída do loop é um único DX11 v2 predefinido; se ele falhar em discriminar esse positivo
do controle `1800@875`, encerramos a busca sintética e adotamos probation real/conservadora.**

## Decisão e estado v26 (histórico, supersedido por v27)

O produto não abre mão do sintético: o usuário leigo continua iniciando o Forge em um clique. A
mudança é concentrar custo e rigor no candidato final, sem triplicar a descida da fronteira.

### Contrato obrigatório

`F2_QUALIFICATION_CONTRACT_VERSION = 26` exige, na mesma execução e no mesmo par exato:

1. `Texture` através de Vulkan explicitamente selecionado;
2. `Dx11Game` através do novo DX11 v2 nativo;
3. `Dx12Game` através do mesmo plano `V8Texture` em DX12 explicitamente selecionado;
4. `Endurance` contínuo.

Qualquer `Fail` físico rejeita o par. Qualquer `Inconclusive`, lane ausente, backend indisponível,
golden inválido, checksum insuficiente, telemetria insuficiente ou baixa residência bloqueia a
publicação sem fabricar condenação do silício. Evidência positiva anterior ao contrato 26 não
desbloqueia Apply.

### Equivalência e diferenças honestas

Vulkan e DX12 são byte-for-byte o mesmo plano WGSL/wgpu, com duração igual e goldens stock próprios
por backend. O DX11 v2 cobre os mesmos domínios funcionais com recursos nativos: target 1536×1536,
textura amostrada, alpha/ROP, depth D24, pixel ALU, compute/UAV de 65.536 elementos e copy/readback.
Dezesseis frames render+compute são enfileirados entre verificações; o antigo `Flush`/wait por frame
foi removido. Cada janela exige checksum do framebuffer e do buffer compute.

Os três lanes são offscreen/headless para manter execução automática e comparável. Portanto este
patch **não alega** reproduzir swapchain/`Present`, Reflex ou o scheduler completo do Overwatch.
Process isolation também continua pendente; um bloqueio que não retorna ainda depende de TDR/Safe
Loop/reboot. Essas limitações são parte do critério de saída, não justificativa para criar variantes
infinitas.

### Tempo e ordem

- controle stock: Vulkan, DX11 v2 e DX12 por 60 s cada antes de qualquer candidato;
- Standard: 120 s por API + 300 s Endurance, cerca de 11 min por par antes do overhead;
- Long: 300 s por API + 1.200 s Endurance, cerca de 35 min por par antes do overhead;
- ordem fixa: Vulkan → DX11 v2 → DX12 → Endurance;
- ETA e execução consomem a mesma ladder finita de quatro dwells.

### Validação e regra final de parada

A suíte passou com 588 testes e dois smokes de hardware ignorados por padrão. Executados
explicitamente em stock, passaram tanto o smoke DX11 v2 render+compute quanto a captura de golden
com backend forçado Vulkan/DX12. Isso valida implementação e seleção de API, não sensibilidade ao
undervolt.

O próximo e último gate físico dessa hipótese é o bake-off com stock/`1800@875` e o positivo rígido
`1860@868`, após reconciliar o boot flag. Se a matriz v26 aprovar `1860@868`, o resultado obrigatório
é **NO GO para precisão sintética nessa geração**: documentar o falso negativo, não criar v27 e usar
fronteira conservadora mais probation de campo. Assim a matriz preserva o objetivo de um clique sem
reabrir o loophole de algoritmo e teste infinitos.

## Estado final v27 depois da calibração em hardware

O bake-off respondeu à pergunta central, mas não com um detector binário perfeito:

| Ensaio | Resultado |
|---|---|
| `1800@875`, DX11 v2 residente, 420 s | passou em 420,248 s; 123.872 frames; 7.742 checks; sem evento `nvlddmkm` |
| `1860@868`, ensaio anterior de 420 s | primeiro `nvlddmkm-153` por volta de 347 s e cascata até bugcheck `0x116`; timing praticamente igual ao Overwatch |
| `1860@868`, repetição v27 de 420 s | passou em 420,230 s; 93,79% das amostras em 1860 MHz; sem TDR |
| `1860@868`, repetição estendida de 600 s | passou em 600,247 s; 178.336 frames; 11.146 checks; sem TDR |

Isso prova duas coisas ao mesmo tempo: DX11 v2 é o sintético mais relevante encontrado, porque já
reproduziu a falha de campo no mesmo horizonte; porém a manifestação é estocástica e uma passagem
limpa, mesmo maior, não prova estabilidade universal. Aumentar 420 para 600 s adicionou três minutos
por perfil e continuou produzindo falso negativo. Repetir até falhar recriaria exatamente o loop
infinito que esta auditoria precisava encerrar.

### Política de produto adotada

1. A descida continua curta e provisória: PowerRender + Texture por bin, sem multiplicar por API.
2. A fronteira aprendida recebe 12 mV de margem e snap para cima em um bin físico real. A evidência
   atual não autoriza reduzir essa margem.
3. Somente os pares finais deduplicados pagam o gate v27: DX11 v2 420 s → Vulkan 120/300 s → DX12
   120/300 s → Endurance 300/1.200 s.
4. Falha física rejeita o par e aciona reparo vertical/ressíntese; inconclusivo bloqueia publicação
   sem inventar condenação do silício.
5. O orçamento é finito: não há repetição automática até obter falha ou aprovação estatística.
6. No primeiro TDR, Sentinel entrega o evento ao dono do teste, solicita cancelamento cooperativo,
   preserva a atribuição e bloqueia novas mutações até reiniciar o Windows. A UI não oferece
   `Recover & continue` na mesma sessão; explica apenas que o ponto e o aprendizado foram salvos.

O resultado comercial honesto é uma qualificação sintética **mais forte e limitada**, com margem e
recuperação projetadas para conter falsos negativos. Não é certificação matemática de estabilidade e
não pode prometer zero TDR; o que o programa pode prometer é que não insistirá no mesmo ponto nem
deixará o usuário leigo preso em troubleshooting depois do primeiro reset de driver.
