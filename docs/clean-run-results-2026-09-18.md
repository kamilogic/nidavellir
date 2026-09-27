# Clean Run interrompida por falha de recuperação do driver — 18/09/2026

Run `f2-forge-1789720117824`, Standard / Clean, iniciada às05:28:37 (UTC−3).
Executável release datado05:28:01; checkpoint contém os novos campos de busca por clock.
Esta investigação é somente leitura dos dados operacionais. Não iniciou Core, reset ou carga.

## Evidência e sequência

| Horário local | Evidência |
|---|---|
| 05:28:37 | Manifesto inicia a run Clean. |
| 05:39:53 | 1920@893 falha por SilentError; último bin aprovado na triagem900mV. |
| Antes de05:43 | 1905@893 também falha por SilentError; último aprovado900mV. |
| 05:47:10 | Texture/Frontier de1890@875 conclui com pass, stock e BootFlag limpos. Isso não é a matriz final de Apply. |
| 05:47:13 | BootFlag arma1890@868, offset300MHz, fase `f2_undervolt_probe`. |
| 05:47:25 | Último checkpoint continua na qualificação Texture30s desse candidato. |
| 05:47:34.582 | Primeiro evento153 `nvlddmkm` da sequência da falha. |
| 05:47:36.624 | Safe Loop e ledger registram CandidateCrash rígido1890@868, contrato31. Sentinel solicita parada cooperativa. |
| 05:47:38 | Sentinel registra outra solicitação de parada; seguem eventos153 e14 do driver até05:47:50. |
| 05:48:48 | Último heartbeat persistido. Não há encerramento confirmado da transação ativa. |
| 05:49:04 | Windows inicia o boot atual. |
| 05:49:20 | WER1001 confirma bugcheck0x116; WER1019 aponta `nvlddmkm.sys` como driver possivelmente relacionado. |

EventLog6008 informa desligamento inesperado às05:48:21. Esse horário antecede o último
heartbeat; não é usado como cronômetro exato da falha. A sequência driver→incidente→bugcheck
é sustentada pelos eventos independentes e pela identidade exata da transação.

## Interpretação

O reinício ocorreu após uma falha do subsistema gráfico durante a exploração de1890@868.
O código0x116 é VIDEO_TDR_FAILURE: falhou a recuperação do driver de vídeo após timeout.
[Referência Microsoft](https://learn.microsoft.com/en-us/windows-hardware/drivers/debugger/bug-check-0x116---video-tdr-failure).
A coincidência com a descida de875 para868mV sustenta a hipótese de instabilidade do candidato;
não prova, sozinha, que subtensão seja a única causa ou que inexista defeito de driver/carga.

O detector identificou erros anteriores sem reinício e preservou sua blacklist. Para a falha
fatal, a atribuição e a condenação também foram persistidas antes do reboot. Porém, não existe
confirmação de retorno a stock/limpeza do BootFlag desse candidato. O pedido de Stop não é
prova de que o worker conseguiu sair da chamada de GPU; tampouco permite afirmar qual chamada
ficou presa. O dump informado pelo Windows é `C:\Windows\Minidump\091826-16593-01.dmp`;
a leitura foi negada pelas permissões desta sessão, portanto sua pilha não foi analisada.

## Resultado e limites

### Revisão do passe1890@875, solicitada pelo usuário

Os registros desse par contêm Discovery de10,065s e Texture/Frontier de33,879s, ambos Vulkan.
No Texture, média/p5/p95 de clock1890MHz,66 leituras de tensão com mínimo/média/máximo875mV,
267 checksums e1 verificação compute. O sampler usa NVML para clock e leituras de tensão
NVAPI; os campos de medição são distintos do alvo solicitado. Isso sustenta que o par foi
exercitado na triagem, sem estabelecer sua estabilidade prolongada ou em outras cargas.

O veredito `validated` é da etapa curta, não do perfil. Não houve DX11, DX12 ou Endurance
final nesse par. A possibilidade de triagem permissiva merece revisão de cobertura/trabalho
verificado, mas a divergência em relação ao1800@875 manual não basta para provar falso passe:
essa referência não determina o máximo da placa e não deve virar teto embutido no algoritmo.
Nenhuma alteração de detector, limiar ou dado operacional foi feita nesta revisão.

-38 observações concluídas:22 Discovery,16 Frontier;30 validated,6 power_bound_clock_drop,
  2 silent_error. Todas essas38 têm cleanup confirmado; o candidato fatal não tem observação
  concluída e está representado por BootFlag, incidente e ledger. Não sintetizar um dwell ausente.
-Máxima temperatura nas observações concluídas71°C; não há telemetria final de868mV que
  permita excluir aquecimento, excursão de clock ou outro fator no instante fatal.
-Nenhum perfil foi publicado. Não chegou à comparação econômica nem ao gate final DX11/
  Vulkan/DX12/Endurance. Essa run não valida ainda as correções de seleção dos três perfis.
-Manifesto Clean informa retenção de negativos, conforme contrato; os dados atuais têm apenas
  as falhas desta run. Isso não é prova independente de qual botão de reset foi usado antes.
-Na inspeção às15:14–15:18, nenhum processo Core estava ativo. `running:true` no checkpoint
  é um estado interrompido em disco, não uma run ainda executando. Recuperação segue pendente.
-Há outro bugcheck0xDE às05:01, anterior ao início desta run; não foi atribuído a ela.

Próxima investigação: caminho de cancelamento/saída da carga e confirmação de stock após o
primeiro TDR. Não prometer que código em espaço de usuário impede todos os bugchecks do driver.
Preservar esta evidência antes de qualquer novo Full Reset; nenhuma recuperação/Resume foi
executada automaticamente nesta análise.

Snapshot, eventos Windows com XML, resumo das observações e hashes:
`target/beta/restart-investigation-20260918-151401/`.
