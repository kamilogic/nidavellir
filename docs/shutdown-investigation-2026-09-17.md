# Core: demora ao fechar depois da run — 2026-09-17

## Evidência observada

O usuário informou que X e Ctrl+C não fechavam o Core e que o programa ficava mais lento
depois da run. O Core antigo, PID10272, estava aberto desde05:15:43 local; a run terminou
às05:55. Ao investigar o fechamento:

- Último heartbeat às14:15:07.523 e marcador de encerramento limpo às14:15:10.017 local.
- Às14:18:17, CIM/performance ainda enumeravam o processo com uma thread e675 handles;
  CPU89%, inteiramente reportada em modo privilegiado,0% em modo usuário.
- A comunicação IPC já não respondia. Get-Process já não encontrava o PID, apesar da
  enumeração via CIM/performance; isso limita a interpretação do estado de término.
- O usuário relatou que o processo finalmente fechou sozinho. Ausente na consulta às14:29:53.
  Há evidência de persistência por mais de três minutos após o marcador, sem duração exata.

Não foi obtida uma pilha da thread. O consumo em kernel é compatível com a lentidão durante
esse intervalo, mas não identifica seu componente causador nem explica toda a lentidão da UI.
Não encerramos o Core à força nem iniciamos outra run.

## Defeito corrigido

Onze locais do wrapper chamavam NvAPI_Initialize repetidamente, incluindo a leitura de tensão
e cada consulta da curva. A dependência Rust encaminha cada chamada à API nativa. Segundo a
[documentação NVIDIA](https://docs.nvidia.com/nvapi/group__nvapifunctions.html), cada inicialização
incrementa a contagem de referências, mesmo se já inicializada, e deve ser pareada com unload.
Não havia liberação correspondente no encerramento de produção. Não se presume uma alocação
de memória por referência; o defeito comprovado é o desequilíbrio do ciclo de vida.

Agora as chamadas compartilham uma inicialização bem-sucedida. O encerramento libera essa
referência uma vez, depois de fechar a admissão, aguardar os workers e bloquear leitores e
Sentinel. A liberação fica dentro do prazo existente de limpeza e precede o marcador limpo.
Falha de unload permanece falha em chamadas subsequentes; o runtime fechado não reabre.
Uma inicialização que falha pode ser tentada novamente sem contabilizar referência inexistente.

O marcador de limpeza não garante que o Windows já terminou o processo: a
[documentação Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess)
explica que I/O pendente pode atrasar esse término. A estratégia existente de saída do processo
não foi alterada. A relação causal entre o defeito NVAPI e a thread presa ainda requer confirmação.

## Verificação e entrega

- 719 testes do workspace passaram; três testes físicos ficaram ignorados na suíte padrão.
- Regressão concorrente:80.000 acessos simulados, uma inicialização, uma liberação; falhas de
  init/unload e rejeição de acesso posterior também cobertas.
- Teste físico separado, somente leitura:128 leituras de tensão, unload e saída do subprocesso
  passaram em123.0905ms. Não aplicou clocks/tensões, não gerou carga nem iniciou o Core.
- Clippy passou com avisos existentes; build release passou. Core e sidecar têm SHA256 idêntico:
  `0797CA2592C3864F8D7E471F2887E8B9746E25D6D99B8CF7345AC709F2410180`.
- Incluídas também as correções anteriores de diagnóstico/decisão de clock excedido. O fluxo
  de desenvolvimento pelo BAT continua válido, sem reinstalação de pacote.
- Fontes anteriores, patches incrementais, logs e manifesto: `target/beta/shutdown-lag-20260917/`.

**Aceitação pendente:** executar a próxima run manual limitada, examinar os novos diagnósticos
do algoritmo e confirmar o fechamento após a carga completa. O teste curto não reproduz horas
de uso ou o estado exato do incidente. Se voltar a travar, capturar pilha/trace enquanto ainda
estiver preso, antes de atribuir o problema ao driver ou ampliar mudanças.

## Recorrência e correção do console — 2026-09-17 à noite

O operador confirmou que o Core voltou a demorar para fechar pelo X mesmo depois de mostrar
`shutdown complete` e a liberação do NVAPI. Portanto, a correção de referências NVAPI não
demonstrou resolver a saída depois da run completa. PID17032 apareceu na primeira consulta
CIM desta investigação, mas desapareceu antes da coleta de threads/performance/pilha.
O marcador limpo foi gravado às23:02:40 UTC, após o último heartbeat às23:02:38 UTC.
Não foi medido o intervalo exato entre o marcador e o desaparecimento do processo.

Foram corrigidos dois caminhos verificáveis:

- O lançador de desenvolvimento abria PowerShell elevado com `-NoExit` e `cargo run`.
  Agora compila antes da elevação e abre diretamente o executável do Core com `RunAs`.
  A janela não depende de um PowerShell pai continuar aberto depois da saída do Core.
- O callback escrevia logs antes e depois da limpeza supervisionada, fora do prazo.
  Esses logs foram removidos do callback: escrita bloqueada no console não pode impedir
  o início da supervisão nem a chamada de saída nativa. Logs internos da limpeza continuam
  dentro da operação supervisionada. X/logoff/shutdown usam4s; Ctrl+C/Break mantêm30s.
  A [documentação Microsoft](https://learn.microsoft.com/en-us/windows/console/handlerroutine)
  informa prazo padrão de5s para `CTRL_CLOSE_EVENT` e alerta que funções do console podem
  não funcionar de forma confiável durante seu fechamento. O sistema pode conceder menos
  tempo em logoff/shutdown; não se promete concluir recuperação nessas condições.

8 testes de encerramento passaram, incluindo subprocessos que invocam o callback real com
um escritor de logs que bloqueia indefinidamente.9 casos simulados executaram o lançador real:
debug/release e modo normal/validação, falha de build, processo concorrente antes/depois do
build, serviço instalado e executável ausente. Nenhum desses testes inicia o serviço ou toca
a GPU. Release e sidecar foram reconstruídos e têm SHA256 idêntico; também incluem Full/Soft
Reset, cuja entrega estava bloqueada pelo Core antigo. Evidência e fontes anteriores:
`target/beta/console-close-20260917/`.

**Limite:** as mudanças eliminam retenção pelo lançador e bloqueio de console fora do prazo;
não comprovam corrigir a persistência durante término nativo. `TerminateProcess` continua
inalterado. A [Microsoft](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/nf-processthreadsapi-terminateprocess)
documenta que I/O pendente pode adiar o término; sem pilha não se identifica o responsável.
Não houve serviço novo, carga GPU, reset ou término forçado. Próxima aceitação é observar
o fechamento normal após a próxima run do operador e capturar pilha/trace imediatamente
caso persista, antes de ampliar alterações de ciclo de vida do driver.
