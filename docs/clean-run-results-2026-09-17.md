# Clean Run de 17/09/2026 — análise de DX11 v4 / Apply v31

## Resultado observado

- Run `f2-forge-1789633001432`, de 05:16:41 a 05:55:15 (UTC−03), duração 38 min 34 s.
- Terminou `incomplete`: nenhum perfil definitivo, nenhum Apply publicado. A busca parou no
  segundo candidato de Apply, ainda em DX11; Vulkan, DX12 e Endurance de Apply não foram executados.
- 54 observações: Discovery com 26 validadas e 4 limitadas por potência; Frontier com 22
  validadas; Apply com 2 inconclusivas. Aprovações das primeiras etapas não qualificam perfis.
- Todas as 30 medições Discovery tiveram 8 leituras de tensão. A falha anterior de amostragem
  escassa não apareceu nesta run.
- Nenhum SilentError, DeviceLost, instabilidade ou TDR/crash registrado nessas 54 observações.
  Máxima registrada: 73 °C. Todas registram retorno a stock e remoção do BootFlag com sucesso;
  nenhuma escreveu blacklist. Na coleta, BootFlag ausente e Safe Loop persistido em `idle`.
  Isso descreve evidências de recuperação; não é uma nova inspeção física da curva atual.
- O serviço PID 10272 continuava aberto na coleta, mas a run estava encerrada.

## Os dois candidatos finais

| Apply | Cobertura ativa observada | Exposição estimada exatamente no alvo | Clock máximo agregado | p99 energético | Resultado |
|---|---:|---:|---:|---:|---|
| 1890 MHz @ 943 mV | 73,239 s | 19,062 s / 26,03% | 1890 MHz | 199,795 W | Exposição insuficiente; roteamento excluiu o par por potência |
| 1875 MHz @ 937 mV | 76,524 s | 30,007 s / 39,21% | 1890 MHz | 200,006 W | Excesso de clock durante trabalho; busca inteira encerrada |

Ambos completaram as cinco fases de carga em aproximadamente 420 s. Houve respectivamente
11.738 e 11.765 verificações de render e o mesmo número de verificações de compute, sem erro.
A tensão máxima medida ficou no respectivo limite de 943/937 mV.

O segundo candidato satisfez os requisitos numéricos de exposição atuais (60 s observados,
30 s no alvo, 35% de fração), mas `upper_clock_exceeded=true` impediu a aprovação. Os 30,007 s
ficam apenas 7 ms acima do mínimo: uma passagem marginal pela estimativa, não evidência de
repetibilidade. A contagem usa amostras em intervalos de trabalho submetido/fence e tensão
válida <= âncora; não é uma captura contínua do clock físico nem uma prova universal de estabilidade.

## O que melhorou e o que ainda bloqueia

1. **O fallback energético foi corrigido neste percurso.** Após rejeitar 1890@943, o algoritmo
   escolheu 1875@937, em vez de carregar 943 mV por vários clocks inferiores. Não houve reparo
   com aumento de tensão causado pelas duas inconclusividades finais.
2. **A nova carga conseguiu exposição ativa relevante.** A residência já não foi o impedimento
   numérico em 1875@937. Ainda foi insuficiente em 1890@943; essa run não prova que o protocolo
   funciona em todos os candidatos ou GPUs.
3. **O controle do teto precisa de diagnóstico.** `RealF2Ops::apply_positive_offset` já escreve a
   curva e chama `lock_core_clock_max_mhz(target)`, que solicita NVML min=210/max=target.
   O retorno de sucesso não bastou para garantir a propriedade prometida pelos comentários:
   houve amostra acima do alvo dentro de trabalho. O máximo agregado foi 1890 (+15 MHz).
   Não há contagem, duração, fase ou curva correlacionada ao evento persistidas. Não se pode
   afirmar se foi um pico isolado, vários eventos, deslocamento de curva ou comportamento da
   leitura/driver. Movimento de curva com offset constante foi observado no diagnóstico anterior,
   mas não estabelece a causa deste evento.
4. **O tratamento da ocorrência encerra a busca inteira.** `dx11_residency::refusal` prioriza
   `dx11_upper_clock_exceeded`; `gpu_undervolt` transforma isso em `ExactApplyInconclusive`;
   `gpu_power_sweep` encerra o laço para inconclusivos não reparáveis. O tratamento específico
   de continuar com outro par existe para `TargetUnexercised`, não para excesso de clock.
   Isso protege a publicação, mas deixa todos os perfis incompletos mesmo com outros candidatos.
5. **Há um impedimento energético independente.** A placa tem limite configurado de 200 W e
   a publicação exige margem de 1% (198 W). Ambos ultrapassaram esse limite de publicação.
   200,006 W não prova falha física ou defeito do limitador. Mesmo sem o excesso de clock,
   1875@937 seria excluído pela regra energética atual. Não basta retirar o veto de clock.
   A admissão antes de Apply usa 200 W, enquanto a publicação usa 198 W; por isso o primeiro
   candidato foi admitido apesar de sua calibração anterior de 198,045 W. É uma distinção
   deliberada entre etapas no código, a revisar por custo/objetivo, não um arredondamento oculto.

## Próximas correções, em ordem

1. **Tornar a violação do teto diagnosticável e verificar o controle efetivo.** Persistir por fase
   máximo ativo, contagem e cobertura temporal estimada das amostras acima do alvo, primeiro
   evento e contexto de tensão/temperatura; capturar curva/offset e configuração de controle em
   diagnóstico limitado quando possível. Separar amostras ativas das ociosas. Não adicionar
   simplesmente outra chamada ao lock já existente, nem supor que sucesso da API prova o teto.
2. **Dar um resultado específico para controle fora do alvo.** Manter o par sem aprovação e sem
   atribuição de instabilidade/blacklist. Após recuperação confirmada, permitir uma decisão
   limitada e explícita sobre outro candidato; recorrência indicando falha geral de controle
   deve parar com motivo acionável. Nunca repetir indefinidamente, aumentar tensão por essa
   ocorrência ou apenas aceitar +15 MHz para fazer o teste passar. Uma correção do controle
   exige nova qualificação da configuração efetivamente aplicada.
3. **Preservar todos os motivos e encerrar a avaliação energética de forma coerente.** Registrar
   excesso de clock, exposição e potência independentemente do motivo prioritário. Avaliar uma
   triagem pesada curta antes dos sete minutos, com evidência suficiente para a mesma regra de
   potência; não promover uma amostra curta a estabilidade ou rejeição física. Definir
   explicitamente se a margem de publicação continua comum aos três perfis, sem alterá-la só
   para aprovar os pontos desta placa.
4. **Validar até o fim depois dessas correções.** Uma próxima execução deve ter limite e critérios
   de parada, comprovar teto/exposição e passagem aos outros backends antes de outra noite inteira.
   A qualidade dos três perfis e a comparação com 1800@875 permanecem não demonstradas.

## Evidências e limites desta análise

Snapshots, auditoria, resumo e hashes: `target/beta/clean-run-20260917/`.
Build registrado nas observações: `069af31eb29fde958c15fbb1520b5b3e02f8bf31-dirty-0f407fe23ca7`.
Fingerprint DX11: `dx11-game-v4/active-residency-heavy-variable`, contrato Apply 31.
Executável em disco hash `81764283218068EB73222387656858D44A29D4BF8E1D0E137ABB71C69D723F77`,
igual ao manifesto da implementação anterior. Não foi inspecionada a imagem do processo em memória.

Análise somente: nenhuma alteração de algoritmo, GPU, serviço, autorização, Reset ou nova run.
Os testes anteriores de software não substituem a aceitação física ainda pendente.

## Correções implementadas após autorização do usuário

- Acrescentado `active_target.diagnostics`, com máximo ativo e contagem por fase, amostras acima
  do alvo e sua cobertura temporal estimada em microssegundos, primeiro evento com tensão e
  temperatura. Leituras opcionais da curva/offset da âncora são posteriores à amostra, limitadas
  a cinco ocorrências brutas por etapa; podem estar ausentes e não são tratadas como simultâneas.
  Tempo estimado não significa duração física contínua. Amostras ociosas não viram violação ativa.
- Excesso de clock em etapa completa e com recuperação confirmada ganha resultado próprio.
  O primeiro par sai da seleção sem reparo de tensão nem blacklist. Uma alternativa é permitida;
  recorrência em um segundo par encerra a run com explicação e pedido de exportação do relatório.
  Resume reconstrói os pares excluídos e o orçamento, sem renová-lo por reinício.
- Potência, falta de exposição e excesso de clock ficam registrados separadamente. Potência
  também insuficiente não oculta a recorrência do controle. Falhas físicas, de recuperação e
  motivos conflitantes de telemetria não são convertidos em continuação permissiva.
- O teto de publicação permanece em 99% do limite da placa. Não foi introduzida triagem curta:
  os dados atuais não estabelecem duração menor equivalente ao critério energético existente.
- 717 testes do workspace passaram; dois testes físicos já existentes ficaram ignorados.
  Clippy core/service em todos os targets passou com avisos existentes. Regressões cobrem
  o caso1875@937, motivos simultâneos, falhas prioritárias, exclusão de idle e orçamento no Resume.
- Evidências e fontes anteriores: `target/beta/clock-control-20260917/`.
  O serviço antigo10272 permaneceu aberto; release e sidecar não foram substituídos. O BAT
  recompila o release ao ser iniciado depois que o serviço anterior for fechado.

**Limite:** estes ajustes corrigem diagnóstico e decisão de busca. A causa física da ultrapassagem
ainda depende da próxima medição limitada; não houve nova carga ou alteração de controles da GPU.
A API usada solicita um intervalo de clocks ([referência NVIDIA](https://docs.nvidia.com/deploy/nvml-api/api/group__nvmlDeviceCommands.html));
a medição desta run mostrou por que a resposta de sucesso, isoladamente, não demonstra contenção.
