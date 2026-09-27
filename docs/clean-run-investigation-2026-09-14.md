# Investigação da Clean Run de 14/09/2026

Atualizado em 16/09/2026. Run original `f2-forge-1789410879755`, build
`069af31eb29fde958c15fbb1520b5b3e02f8bf31-dirty-0ef50ca0cca1`, RTX 3060 Ti,
driver 610.88. Evidências em `target/beta/clean-run-20260914/`.

## Conclusão e limite da evidência

A causa imediata dos resultados finais Inconclusive está confirmada: o DX11 não
atingiu a cobertura mínima na frequência solicitada. Isso não foi uma sequência
de erros de cálculo ou crashes. A investigação encontrou uma pausa evitável da GPU
durante os checksums de CPU e repetição de candidatos que já excedem o envelope
energético de publicação. Ambas foram corrigidas; a comparação física não aprovou
o ponto de 1830 MHz / 943 mV e não comprova que toda a Forge esteja resolvida.

Writer, 0 MHz de tolerância, residência mínima de 35% e histórico de falhas foram
preservados. A cadência de carga mudou, portanto Exact Apply passou a v30 e DX11 a
v3; positivos antigos não aprovam a nova carga. O piso do histórico CandidateCrash
permanece v29 para que a mudança não apague cones ou orçamento de segurança.
As duas janelas diagnósticas terminaram, o serviço está encerrado e o heartbeat
permanece PAUSED. Não iniciar uma terceira janela ou uma Forge automática.

## Correção DX11 v3 e segunda janela — comparação encerrada

O DX11 v2 esvaziava a fila da GPU e calculava em CPU o checksum de 9.437.184 bytes
antes de enviar os próximos 16 frames. Uma reprodução somente em CPU, compilada
em release, mediu **7,718 ms por checksum**: cerca de 23,3% dos 33,143 ms por lote
da primeira janela. O controle stock curto registrou utilização média de 77,7%.
O DX11 v3 mantém no máximo um lote em execução enquanto a CPU confere a cópia
anterior, já sincronizada em staging. Render e compute continuam verificados a
cada 16 frames. O cancelamento drena e verifica o lote final; uma falha de checksum
drena o trabalho pendente antes do reset.

A segunda e última janela autorizada terminou em 15/09 às 22:02:29Z, exit code 0.
Foram mantidos ponto, writer, 120 s de preheat stock e 420 s de dwell; mudou a
cadência DX11. As execuções ocorreram em horários diferentes, sem controle da
temperatura ambiente. O aumento de temperatura/carga faz parte do resultado,
não de uma comparação com temperatura idêntica.

| Medida | DX11 v2 | DX11 v3 |
| --- | ---: | ---: |
| Dwell de render | 420.016 ms | 420.035 ms |
| Frames / checks render e compute | 202.768 / 12.673 | 261.424 / 16.339 |
| Clock médio / p5 / p50 / p95 | 1767 / 1665 / 1785 / 1830 MHz | 1651 / 1620 / 1650 / 1680 MHz |
| Residência em 1830 MHz | 5,253% | 0,052% |
| Potência média / p99 | 174,54 / 180,102 W | 199,13 / 199,911 W |
| Temperatura máxima | 71 °C | 76 °C |
| Utilização média na instrumentação de 500 ms | não coletada | 98,85% |
| Erro de integridade / TDR / device loss | nenhum | nenhum |
| Resultado / reset / BootFlag | Inconclusive / OK / limpa | Inconclusive / OK / limpa |

O throughput aumentou **28,9%**, sem remover verificações. A carga agora encosta
numericamente no limite de 200 W; a residência piorou, e o ponto continua inelegível.
Os 98,07% da flag SW_POWER_CAP na primeira janela **não provavam** saturação de
potência: seu p99 era apenas 180,102 W. A segunda janela tem evidência numérica
de saturação; ainda não isola todas as causas da baixa residência anterior.

Os 42 readbacks da segunda janela mantiveram +90 MHz na âncora. A base lida variou
de 1740 para 1725 MHz, com frequência efetiva de 1830 ou 1815 MHz. Essa variação
de 15 MHz existe, mas não explica sozinha o p95 de 1680 MHz. Não ajustar o writer
ou relaxar residência com base neste resultado.

O fluxo de Apply agora verifica o p99 após o primeiro DX11 completo, reset-clean
e com checks/telemetria válidos. Se já excedeu o teto de publicação existente
(198 W numa placa de 200 W), retorna `ExactApplyPowerCeilingExceeded`, preservando
a observação original Inconclusive. Evita os dois dwells idênticos adicionais
(14 minutos), as APIs restantes e reparo por aumento de tensão nesse clock.
Não grava blacklist. O motor pode buscar um clock inferior, que ainda precisa
ser qualificado integralmente. Flag isolada, pico isolado, cancelamento,
telemetria ausente, falha de integridade ou reset não acionam esse caminho.

Validação: **704 testes Rust passaram**, dois testes de hardware ficaram ignorados
na suíte comum; o smoke real DX11 em stock foi executado separadamente e passou,
incluindo goldens alterados e cancelamento. O roteamento novo foi validado em
software com os números medidos; não houve uma Forge completa após essa alteração.
Journals copiados e comparados por hash em `point-baseline/journal.jsonl` e
`point-pipeline/journal.jsonl`; consolidação reproduzível em `investigation-summary.json`.
O executável da segunda janela teve SHA-256
`DFE3CBBFD7483D9F56B724D0A410ABBBF1B602354F00AE8D85163188361CC667`.
O executável final também inclui o roteamento por potência, implementado depois
dessa janela; seu hash fica no handoff e em `pipeline-final-release-build.log`.

## Diagnóstico físico limitado de 1830 MHz / 943 mV (15/09/2026)

Foi executada uma única transação explicitamente autorizada, fora de uma Forge:
preheat stock de 120 s, aplicação do ponto exato e um dwell DX11 completo de
420 s. A execução terminou normalmente às 06:32:20Z, antes de qualquer outra
etapa da matriz. Journal: `C:\\ProgramData\\Nidavellir\\development-validations\\11304-1789453361653770900.point.jsonl`.

| Medida | Resultado |
| --- | ---: |
| Dwell DX11 | 420 016 ms (≈420 s) |
| Frames / checks | 202.768 / 12.673 |
| Residência em 1830 MHz | **5,253%** (mínimo 35%) |
| Clock médio / p5 / p50 / p95 | 1767 / 1665 / 1785 / 1830 MHz |
| Potência média / fração limitada | 174,54 W / **98,07%** |
| Temperatura máxima | 71 °C |
| Integridade, TDR e device lost | nenhum |
| Reset / BootFlag | confirmado / desarmada |

O resultado contratual continua `Inconclusive: target_residency_low`, mas agora
com readback alinhado. Antes da escrita, 943 mV era 1740 + 90 = 1830 MHz e os
offsets superiores eram zero. Depois da escrita e em todas as 42 amostras de
dwell, o driver reportou 1830 MHz nos pontos de 943 e 950 mV (943 com offset
efetivo de +90 MHz) e 1785 MHz em 937 mV. Portanto, a queda planejada do
plateau não aparece como a curva efetiva lida durante o dwell. A diferença entre
o formato aritmético e os comentários do planner é reproduzível; um defeito
físico do writer não foi demonstrado. A flag de potência isolada também não
permite atribuir esta falha ao limite de potência, pois o p99 ficou em 180,102 W.

O diagnóstico voltou a stock, sem perfil persistido, com `safe_loop.json` em
`idle`, `pending_forge_incident: null` e nenhum processo do serviço ativo. Não
foi iniciada outra carga naquele momento. A segunda janela posterior está acima.

## O que ocorreu na run

A run começou às 18:34:39Z e terminou pausada às 23:38:41Z por solicitação do
operador. Stop cooperativo, reset confirmado, BootFlag desarmada e autorização
de desenvolvimento consumida estão no snapshot/auditoria finais. Não houve perfil
qualificado. O checkpoint foi preservado no momento da parada; qualquer Reset
manual posterior não altera o conteúdo das cópias desta investigação.

Das 103 observações preservadas:

| Etapa | Resultado | Quantidade |
| --- | --- | ---: |
| Discovery | stable | 31 |
| Discovery | clock_drop | 9 |
| Frontier qualification | stable | 24 |
| Exact Apply / DX11 | qualification_inconclusive | 39 |

As 39 tentativas finais cobrem 13 pares de frequência/tensão: 38 completaram
420 segundos e a última foi interrompida pelo Stop. Somaram **272,08 minutos**
de DX11. Todas registram `target_residency_low`, sem silent_error, device_lost,
unstable ou tdr_or_crash. A permanência no alvo foi de **0,020% a 1,799%**; o
contrato exige **35%** e tolerância de frequência **0 MHz**.

O percentil 95 do clock ficou 15 MHz abaixo do alvo em 24 tentativas, 30 MHz
abaixo em seis e 45 MHz abaixo em nove. Isso descreve o percentil 95, não a média
ou o clock constante. Por exemplo, 1830 MHz / 943 mV teve p95 de 1815 MHz e
médias de aproximadamente 1756, 1756 e 1764 MHz.

O motor repete até três dwells e transforma três resultados homogêneos em
`DX11StructuralClockDrop`, tenta reparo de tensão e/ou reduz o alvo. Cada trio
completo custa 21 minutos antes da próxima alternativa. A repetição é finita,
mas procura outros pontos sem distinguir uma limitação do candidato de uma
divergência sistemática da aplicação da curva. Nenhuma tentativa chegou às
etapas seguintes da matriz final: Vulkan, DX12 e Endurance.

Fontes no código: `dx11_qualification_coverage_from_run` em
`crates/service/src/gpu_power_sweep.rs`; `exact_apply_dx11_structural_clock_drop_attempt`,
`exact_apply_dx11_structural_clock_drop_complete` e o ramo de repetição em
`crates/service/src/gpu_undervolt.rs`.

## Formato da curva: reprodução confirmada em software

O planner `plan_bounded_anchored_positive_offset` eleva somente o ponto âncora.
Nos pontos de tensão superior, escreve `min(target - base, 0)`: reduz os que
estão acima do alvo e deixa intactos os que estão abaixo. Isso pode produzir
um pico seguido de uma queda, apesar de os comentários descreverem um plateau.

Trecho capturado da placa e reproduzido pelo planner real:

| Tensão | Base capturada | Offset planejado | Frequência planejada |
| ---: | ---: | ---: | ---: |
| 937 mV | 1740 MHz | 0 MHz | 1740 MHz |
| 943 mV (âncora) | 1755 MHz | +75 MHz | 1830 MHz |
| 950 mV | 1755 MHz | 0 MHz | 1755 MHz |
| 956 mV | 1770 MHz | 0 MHz | 1770 MHz |
| 975 mV | 1800 MHz | 0 MHz | 1800 MHz |
| 993 mV | 1830 MHz | 0 MHz | 1830 MHz |
| 1006 mV | 1845 MHz | −15 MHz | 1830 MHz |

O trecho contém uma descida de 75 MHz imediatamente depois da âncora e sete
pontos superiores abaixo do alvo. O verificador `verify_anchored_positive_offset`
aceita os offsets planejados: ele verifica o teto e proíbe offsets positivos
fora da âncora, mas não exige continuidade ou plateau no alvo. Os testes
existentes de plateau usam uma elevação de apenas 15 MHz, com todos os pontos
superiores já acima do alvo; esse caso não expõe a queda da tabela acima.

A tensão é travada antes da escrita da curva em `RealF2Ops::apply_positive_offset`.
Portanto, não está demonstrado que os pontos de tensão superior sejam usados
diretamente durante o dwell. A primeira janela posterior capturou um plateau
efetivo em 943/950 mV. O formato aritmético é um achado confirmado, mas não
equivale ao readback físico e não justifica alterar o writer nesta investigação.

Reprodução sem NVAPI/NVML e sem carga, usando o trecho capturado:

```powershell
cargo run -p nidavellir-gpu-stress --example dx11-stock-probe --release -- --replay-plan-1830-943
```

Resultado verificado: `replay=true; descending_edges=1; higher_bins_below_target=7`.
Arquivos: `anchor-plan-1830-943.txt`, `offline-plan-replay.txt` e
`offline-plan-replay-build.log`. O trecho capturado usa base 1755 / offset +75;
as tentativas da run em 1830 / 943 usaram **base 1740 / offset +90**. Não confundir
a captura posterior com o plano histórico completo, que não foi exportado.

## Base variável e limites da verificação

`f2_forge_inputs` lê a base uma vez depois do preheat; a mesma `sane_base_curve`
alimenta os candidatos durante a run. `RealF2Ops::verify` confirma os offsets
com tolerância de 15 MHz e não fornece frequência observada à verificação da
âncora. Assim, `RaiseVerified` confirma a escrita dentro dessa tolerância; não
prova residência no alvo exato exigido pelo DX11. Os logs não preservam o offset
efetivamente lido nem uma curva base por tentativa para determinar qual ocorreu.

Os controles stock mostraram variação na base de 943 mV: 1755 → 1740 MHz em
um controle curto; no controle de 120 s, 1725 antes, 1740 durante e 1725 depois.
As leituras base/live são sequenciais. Durante esse controle a base permaneceu
1740 de 63 a 72 °C: não há evidência de uma simples regra temperatura → −15 MHz.

O teto NVML de clock impede ultrapassar o alvo, mas usa mínimo 210 MHz; não
obriga a GPU a alcançá-lo. Ele não corrige um plano que entregue frequência menor.

## Controles já executados antes desta retomada

Mesma carga DX11, golden próprio em stock, descartando os primeiros seis segundos
da telemetria para esta comparação. Todos completaram verificações de render e
compute sem erro. Estes controles não equivalem à matriz de aprovação.

| Controle | Duração | Amostras ≥1830 MHz | p95 | Temperatura máxima |
| --- | ---: | ---: | ---: | ---: |
| Stock | 30 s | 58,45% | 1890 MHz | 65 °C |
| Stock com teto de 1830 MHz | 30 s | 64,42% | 1830 MHz | 66 °C |
| Stock com leitura de curva | 30 s | 55,80% | 1875 MHz | 67 °C |
| Stock aquecido | 120 s | 29,83% | 1860 MHz | 73 °C |

O controle com teto libera o limite ao terminar; o reset foi confirmado. Ele não
aplicou offset V/F nem trava de tensão. Por isso não isola o efeito dessas duas
operações presentes na run. Os controles curtos mais frios atingiram o alvo;
o controle aquecido ficou abaixo de 35%. Isso impede concluir que basta alterar
um limite para todos os pontos passarem.

A flag SW_POWER_CAP aparece quase continuamente também em stock. Isoladamente,
ela não distingue o problema encontrado. Manter a análise conjunta com potência
numérica; não converter a flag em falha física ou novo veto sem outra evidência.

## Próxima validação

O limite de duas janelas foi consumido. Esta comparação terminou com os dois
resultados Inconclusive preservados e sem perfil. A próxima evidência de produto
será a run manual completa pelo fluxo de comandos já escolhido pelo operador,
com a build atual, preflight e autorização de desenvolvimento próprios. Deve
confirmar que o novo roteamento evita repetições por potência e que algum ponto
consegue concluir as quatro etapas; se nenhum conseguir, registrar a recusa
explicada, sem fabricar aprovação ou repetir o mesmo diagnóstico indefinidamente.

## Integridade dos artefatos

- Fonte principal: `observations-final.jsonl`, 103 linhas, SHA-256
  `E3DA8A84C77D2EE4166D6CECFBAB133E699DBC67096A5B9E3661BBF18AF90571`.
- Auditoria: `authorization-final.jsonl`; estado no Stop: `final.json` e
  `forge-state-final.json`; comandos: `StopPowerSweep-response.json` e
  `ExportForgeLog-response.json`.
- Controles: `dx11-*.csv`; consolidação: `investigation-summary.json`, gerada
  por `summarize-investigation.cjs`.
- Ledger atual conferido, igual à base preservada: SHA-256
  `490A5B753C8E48CFFCFB7747328614B1C44F54B02FC4A7BB0DCFC338A3898802`.
- Alterações desta investigação: replay offline, diagnóstico limitado, DX11 v3,
  Exact Apply v30, rejeição antecipada por potência e piso histórico de segurança
  separado da versão positiva. Writer, thresholds de aprovação e histórico
  negativo foram preservados. As autorizações diagnósticas foram consumidas.
- Conferência pós-execução em 16/09: Safe Loop idle, incidente pendente null,
  BootFlag e perfil aplicado ausentes, serviço encerrado; hashes de Safe Loop e
  ledger iguais aos preservados antes do diagnóstico.
