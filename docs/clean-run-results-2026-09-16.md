# Resultado da Clean Run de 16/09/2026

Run `f2-forge-1789548115932`, iniciada pelo operador via BAT/interface, com aprendizado
`clean_run` e tempos Standard. Início 05:41:55, término 08:49:45 de 16/09 (UTC−3):
**3 h 07 min 49 s**. Resultado persistido: **incomplete**, nenhum perfil qualificado.

RTX 3060 Ti, driver 616.92, build registrada
`069af31eb29fde958c15fbb1520b5b3e02f8bf31-dirty-8fffd65da1db`.
O arquivo release local tem SHA-256
`207C1EB65863218C0088EF48FD8FD1BF13C6F55CB32F2BEAF041ED9DEC2963BC`.

## Evidência preservada

Os 103 registros finais pertencem exclusivamente a esta run. O manifesto registra o
arquivamento do aprendizado anterior antes da busca; nenhuma aprovação depende de
evidência positiva de outra run. Exclusões históricas reais permaneceram ativas.
Não equivale a uma GPU sem histórico de incidentes.

Arquivos copiados sem modificar os originais em `target/beta/clean-run-20260916/`:
checkpoint, observações, auditoria, Safe Loop, ledger e manifesto Clean. As observações
ativas e a cópia arquivada pelo próprio motor são idênticas por SHA-256:
`C30538174748BD768054518803AB774F679A09B7FA20DEC6A699ADBF2084D848`.
`summarize.cjs` reproduz os números em `summary.json`.

| Etapa | Resultado | Quantidade |
| --- | --- | ---: |
| Discovery / calibração | Validated | 32 |
| Discovery / calibração | PowerBoundClockDrop | 5 |
| Discovery / calibração | PowerTelemetryInconclusive | 13 |
| Frontier Texture | Validated | 25 |
| Frontier Texture | Inconclusive: boost_edge_telemetry_low | 7 |
| Exact Apply DX11 v3 | Inconclusive: target_residency_low | 21 |

Todos os 21 dwells DX11 completaram 420 s, somando **147,03 minutos**, em 18 pares.
Nenhum chegou ao Vulkan/DX12/Endurance do gate final. Os passes de Frontier Texture
não são passes da matriz final. A residência DX11 variou de 0 a 0,976%, ante 35% exigidos.

## O que a correção anterior resolveu

Em 17 pares, o p99 DX11 completo ultrapassou 198 W (99% do limite de 200 W). A recusa
energética interrompeu repetições/etapas restantes e fechou o aumento de tensão naquele
clock, sem gravar blacklist. A maioria desses pares terminou após um só DX11.
1725 MHz / 875 mV consumiu dois: 197,998 W na primeira tentativa, 198,367 W na segunda.
Isso confirma o roteamento por potência em uso real, mas não a aprovação de perfis.

## Bloqueios que permanecem

### Coleta insuficiente de tensão e classificação imprecisa

Os **13 PowerTelemetryInconclusive** têm p99 numérico, ausência de thermal throttle,
clock p5/p95 dentro da tolerância de Discovery e tensão medida igual à âncora. Porém,
11 têm somente **duas leituras de tensão**, e os dois últimos têm **uma**.
`enforce_voltage_authority` exige no mínimo três e devolve Inconclusive quando faltam.

O coletor em `gpu_power_sweep.rs` lê tensão a cada **16 iterações** do sampler de sensores;
o intervalo real depende da latência das consultas. Descarta ainda o início do dwell.
Os registros afetados têm 24–38 amostras válidas de clock/potência em cerca de 10 s,
mas somente 1–2 de tensão. O programa exige evidência que sua coleta não conseguiu entregar.

`gpu_f2_sweep.rs` rotula qualquer Inconclusive da etapa Discovery como
PowerTelemetryInconclusive, ocultando a causa de tensão. Não atribuir esses 13 casos a
potência alta ou instabilidade física. Exemplo final: **1710 MHz / 875 mV**, clock
médio/p5/p95 de 1710 MHz, p99 de 170,47 W, tensão 875 mV, apenas uma leitura de tensão.
Essa falha impediu o caminho de reparo e a última alternativa de clock inferior.

Prioridade seguinte: tornar a coleta de tensão compatível com a janela curta e registrar
a razão específica. Manter a exigência de evidência; não promover os registros antigos
nem simplesmente reduzir o mínimo de amostras para fazê-los passar.

### Cobertura curta da fronteira

Sete dwells de Frontier Texture terminaram em `boost_edge_telemetry_low`. É outra dívida
de cobertura a revisar junto do sampler; não há registro de erro de integridade nesses
casos. Os dados resumidos não demonstram que tenham exatamente a mesma causa da tensão.

### Baixa residência DX11 fora do teto de potência

**1710 MHz / 868 mV** completou três DX11 com p99 de 194,719–194,930 W e p95 de clock
1695 MHz (um bin abaixo). Residência de 0,255–0,976%. Gerou DX11StructuralClockDrop,
permitiu um reparo para 875 mV, e esse reparo esbarrou na falta de amostras acima.
O limite energético não explica sozinho esta baixa residência. A run não contém readbacks
de curva alinhados a cada dwell suficientes para atribuir isso ao writer/base térmica.
Não relaxar os critérios de clock por suposição.

## Estado ao fim e limite da inspeção atual

- Temperatura máxima registrada: **75 °C**, sem thermal throttling registrado.
- Nenhum silent error, unstable, device loss ou TDR/crash nos 103 registros.
- Todos registram reset a stock bem-sucedido e BootFlag limpa. Nenhum perfil aplicado
  ou BootFlag persiste; Safe Loop está idle, sem incidente pendente.
- Ledger idêntico ao anterior: SHA-256
  `490A5B753C8E48CFFCFB7747328614B1C44F54B02FC4A7BB0DCFC338A3898802`.
- Autorização encerrada com motivo `incomplete`. Nenhuma nova autorização/run foi enviada.
- Às 15:12–15:20 (UTC−3), o processo do serviço 19348 ainda existia, mas duas conexões IPC
  expiraram antes de conectar. A avaliação usa os arquivos finais/auditoria, não uma resposta
  viva do serviço. A GPU estava a 44 °C e 9% de uso na leitura pontual. Não foi reiniciado ou
  encerrado o serviço durante esta análise. Investigar a falta de resposta separadamente.

O caminho Full Reset/Clean conseguiu começar sem aprendizado positivo e terminar com
evidência preservada, mas **não passou a aceitação de descoberta e publicação de um perfil**.
Esta inspeção não alterou o algoritmo nem iniciou outra carga. Corrigir a coleta e seu motivo
de recusa é o próximo passo concreto antes de outra run longa.

## Correção da coleta implementada em 16/09

O dwell passou a reutilizar uma sessão NVML, evitando a inicialização e o inventário completo
a cada leitura. A tensão agora é consultada por tempo decorrido (500 ms), mantendo pelo menos
três leituras válidas para aprovação. Discovery preserva os 6 s de descarte inicial; qualificação
inclui a tensão das fases ativas desde a abertura. Atrasos não geram amostras artificiais de reposição.

Os registros novos preservam `inconclusive_reason`, distinguindo falta de tensão, tensão acima
da âncora, teto de clock excedido, limitação térmica, ausência de potência e inconsistência de p99.
`DiscoveryInconclusive` separa recusas de medição das recusas específicas de potência. A razão
também aparece no relatório de texto. Registros antigos não são reclassificados nem aprovados.
As fases de qualificação registram `sample_count` e `clock_max`; uma leitura atravessando a
mudança de fase não conta como cobertura de nenhuma das duas fases.

Validação: 707 testes de software passaram, com dois testes de hardware já existentes ignorados.
Uma consulta de sensores de 10 s, sem carga criada ou alteração da GPU, obteve 8 leituras válidas
de tensão após o descarte e 39–40 leituras de clock/potência por janela de 1,2 s. Evidência em
`target/beta/sampler-fix-20260916/`. Isso verifica a cadência nesse ensaio, não sob carga pesada.

Pendente: validar a coleta sob carga e explicar a baixa residência DX11 fora do teto de potência.
O novo máximo por fase melhora o diagnóstico; a tolerância antiga de +15 MHz no p95 não foi
convertida em uma garantia de teto estrito. Os critérios de qualificação não foram relaxados,
nenhum perfil foi aprovado e não houve nova run. O total deste relatório foi corrigido de 83 para
103: as contagens por grupo já estavam corretas e somam 103.

## Segunda run manual: coleta corrigida, cascata de potência identificada

Run `f2-forge-1789586811426`, pausada manualmente pelo operador em 16/09. Evidência preservada
em `target/beta/power-routing-20260916/`: 62 observações, todas com reset/BootFlag confirmados.
As 36 observações Discovery têm oito leituras de tensão cada: 26 validadas nessa etapa e dez
PowerBoundClockDrop, nenhuma inconclusiva por coleta. Frontier: 18 validadas e um SilentError
em 1830@912, preservado como evidência negativa. Temperatura máxima registrada: 75 °C.

As sete qualificações DX11, 1890/1875/1860/1845/1830/1815/1800 MHz a 943 mV, consumiram
49,004 minutos. Todas ficaram inconclusivas por `target_residency_low`, com p99 de
199,71–199,85 W, acima dos 198 W de publicação. Não houve aprovação final de perfil.

O erro de seleção é demonstrável: após a primeira rejeição energética, o fast-drop do Godforge
criava um candidato prioritário com clock menor e os mesmos 943 mV. A calibração aceitava até
200 W, enquanto a qualificação seguinte recusava acima de 198 W. Isso repetiu testes longos
antes de avaliar pares de menor tensão já descobertos, como 1800@900 e sua calibração a 906 mV.
Esses pares tampouco estão aprovados para uso; são alternativas medidas que precisam do gate.

Correção localizada: `f2_godforge_fast_drop_candidate` não cria esse candidato prioritário quando
o motivo é `ExactApplyPowerCeilingExceeded`. A síntese normal retoma a seleção entre candidatos
existentes. Nenhum par vizinho é condenado por inferência, nenhum teto foi aumentado e nenhuma
inconclusividade virou aprovação. O fallback por falha física mantém o comportamento anterior.
505 testes do serviço passaram, incluindo regressão cobrindo os sete targets da cascata.
Código ainda requer nova execução para confirmação em hardware; o processo aberto não foi reiniciado.
O rebuild release foi bloqueado por acesso negado ao executável em uso pelo serviço 20400;
o sidecar não foi substituído. Encerrar normalmente o serviço é necessário antes do próximo build.
Permanecem pendentes a baixa residência DX11 e a aceitação dos perfis completos.

## Comparação controlada de carga — 16/09, 22:09–22:14 local

Com autorização do operador, executado uma única vez `diagnose-f2-loads`, reutilizando a
transação de diagnóstico1830@943. Após goldens e120 s stock, cinco fases de30 s mantiveram
os mesmos offsets aplicados. Nos níveis intermediários, trabalho DX11 verificado em janelas
de100 ms alternou com pausas; os percentuais abaixo são ritmo solicitado, não utilização medida.

| Fase | Ritmo | Amostras exatamente1830 MHz | Potência média | Checks render/compute |
|---|---:|---:|---:|---:|
| Contínua inicial |100%|1,01%|195,56 W|1117/1117|
| Intermediária |75%|42,23%|160,05 W|785/785|
| Intermediária |50%|63,39%|124,75 W|532/532|
| Intermediária |25%|78,42%|88,53 W|270/270|
| Contínua final |100%|0,68%|196,52 W|1166/1166|

Todos os checks corresponderam aos goldens; nenhuma fase cancelada, máximo73 °C, nenhum clock
amostrado acima1830. O flag de potência ficou ativo em98,99% e100% nas fases contínuas.
Isso sustenta a carga como fator dominante da residência baixa neste ponto. A ordem não foi
randomizada e a temperatura variou; não é isolamento completo de cada fator físico.

A leitura da curva confirmou inicialmente base1740+offset90=1830 MHz no bin943 mV. Na transição
da primeira fase, registrou base1725/live1815 com o mesmo offset, retornando depois1740/1830.
Portanto há também deslocamento de um bin na curva viva, sem reaplicação pelo programa.
A leitura auxiliar do lock de tensão retornou `ArgumentExceedMaxSize` durante Apply; preservada
no journal, não apresentada como lock confirmado. A tensão medida nunca excedeu943 mV.

Limitação decisiva: amostras incluem pausas. A coleta de clock/tensão é sequencial, não atômica.
Os percentuais altos não aprovam estabilidade sob trabalho ativo. Antes de mudar a qualificação,
é preciso distinguir intervalos de trabalho/drenagem/pausa e contabilizar exposição ativa real.
Manter a carga pesada para integridade, potência e desempenho; não reduzir35% simplesmente
para transformar estas medições em aprovação.

Diagnóstico terminou deliberadamente Inconclusive/não publicável: não executa o contrato de
qualificação. Reset confirmado, todos os offsets tocados zerados, lock vazio e BootFlag removido.
SafeLoop, ledger e observações mantiveram hashes; checkpoint pausado restaurado byte a byte.
506 testes do serviço passaram. Release/sidecar atuais têm SHA256
`C5E5293E8B9640BC835A628F666C39906A6C36DED283D3DC42877C5F78EE9B0F`.
Evidência completa: `target/beta/load-comparison-20260916/` (journal, summary, scripts, testes,
build e verification). Nenhum serviço ficou rodando e nenhuma segunda tentativa foi iniciada.
