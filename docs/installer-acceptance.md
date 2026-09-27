# D5 — validação do instalador no Windows

O pacote é gerado por `scripts/installer-acceptance.ps1 -Action Prepare` depois do build.
Ele contém o instalador, o manifesto SHA-256, este roteiro e o executor. Preparar ou
inspecionar o pacote não instala o programa nem inicia o serviço.

## PC de uso autorizado

Uma VM é uma opção de isolamento, não um requisito do produto. Com autorização do
operador, use `ExistingPc` para testar no PC existente. Esse modo exige serviço e
registro de instalação ausentes, nenhum processo Nidavellir, nenhum BootFlag,
checkpoint ou perfil para aplicação automática. O histórico real permanece no lugar.
O executor copia todo o ProgramData do Nidavellir para `data-before-install` no
pacote e verifica cada arquivo por SHA-256 antes de instalar. Nenhum registro sintético
é inserido nesse modo; o ledger, incidentes pendentes e blacklist são conferidos entre etapas.

Em uma cópia nova do pacote, primeiro inspecione; para executar, use PowerShell elevado:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\installer-acceptance.ps1 -Action Inspect -Environment ExistingPc
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\installer-acceptance.ps1 -Action Run -Environment ExistingPc -ConfirmHostLifecycle
```

O teste instala, inicia, para, reinstala e remove o serviço. Faz apenas Ping pelo IPC;
não inicia Forge, Apply, ACK ou reset. A partida normal do serviço lê sensores e
executa sua rotina de recuperação, por isso o estado é conferido antes. O serviço
e os executáveis devem estar ausentes ao terminar; dados e backup permanecem.
Arquivos operacionais de heartbeat/Sentinel podem ser atualizados pelo serviço.

Em falha, preserve o relatório, backup e estado atual. Inspecione o serviço/processos
antes de decidir a recuperação; não restaure automaticamente dados antigos sobre
novas evidências de segurança. Para desfazer uma instalação concluída, execute o
desinstalador em `Program Files\Nidavellir Acceptance`; ele preserva ProgramData.
Esse cenário comprova instalação com histórico existente, não um Windows limpo.

## Alternativa: VM descartável

Use uma VM Windows descartável (Hyper-V, VMware ou VirtualBox), sem NVIDIA física
repassada à VM e sem pastas do host mapeadas em ProgramData/Program Files. Copie o
pacote para o disco local da VM. Prepare um snapshot limpo, com WebView2 disponível
ou conexão para o bootstrapper oficial do instalador. Este é o modo padrão do executor.

No PowerShell da VM, dentro da pasta copiada:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\installer-acceptance.ps1 -Action Inspect
```

O relatório informa os bloqueios. A presença de Hyper-V no host não o torna uma VM.
Após conferir o ambiente descartável, execute em PowerShell elevado dentro da VM:

```powershell
powershell.exe -NoProfile -ExecutionPolicy Bypass -File .\installer-acceptance.ps1 -Action Run -ConfirmDisposableVm
```

O executor instala em `Program Files\Nidavellir Acceptance`, confere o caminho do
serviço e seu hash, faz apenas Ping, testa reinstalação com serviço ativo e parado,
Stop pelo SCM e desinstalação. Um registro sintético identificado verifica a
preservação do histórico; nenhum histórico real do host deve ser copiado.
A opção de execução vale apenas para esse processo; não altera a política do Windows.

Cada etapa grava `acceptance-report.json` antes e depois de executar. Em falha ou
interrupção, preserve a pasta/relatório e reverta o snapshot; o executor não apaga
evidências nem força o encerramento de processos para continuar. Não repita numa
instalação parcial. O uninstaller é executado diretamente para acompanhar seu código
de saída; seu próprio arquivo pode permanecer, mas o serviço deve ser removido.
Os argumentos `/S`, `/D=` e `_?=` seguem a
[documentação oficial do NSIS](https://nsis.sourceforge.io/Docs/Chapter3.html).

## Evidência que ainda exige complemento

- Windows limpo; a migração do pacote distribuído v0.3.1 já passou neste PC.
- Falhas de parada sob trabalho real na GPU; a falha de partida por arquivo ausente já foi exercitada.
- UI em hardware não suportado; comunicação real, recuperação exibida e offline/reconexão já passaram neste PC.
- D3 em GPU elegível: qualificação, Apply, medições, uso real, reinício e retorno a stock.

`lifecycle_subset_passed` comprova apenas as etapas nomeadas no relatório. Não fecha
D5 sozinho e não é validação de undervolt. O checklist completo fica em `roadmap.md`.

## Checagem da aplicação instalada — 2026-09-13

Evidências em `target/beta/installed-desktop-acceptance/`: a instalação do pacote
SHA-256 `7524EFB1DE40464F0356B748E1AD70042F7D7EA3775CDCCFD791466A336A301C`
abriu como usuário comum, detectou a RTX 3060 Ti pelo serviço real, mostrou o bloqueio
de segurança e navegou entre Forge e Settings sem erros JavaScript observados.
Os atalhos públicos Desktop/Menu Iniciar apontavam para o executável instalado.

A inspeção usou [Playwright conectado ao WebView2](https://playwright.dev/docs/webview2),
com porta de depuração apenas em loopback e dados de WebView separados. Não houve
simulação de IPC. `native-read-responses.json` contém as respostas reais; o campo
`ipcTrace` vazio no primeiro roteiro não representa ausência de tráfego, pois a
função Tauri é imutável e o observador não foi instalado. Um erro inicial de expectativa
do roteiro foi corrigido uma vez; `assertion-mismatch.json` conserva essa evidência.

A primeira solicitação de Stop teve o UAC cancelado. Na retomada, o operador aceitou
a elevação e a execução final em `target/beta/installed-desktop-remainder/` concluiu
todos os cenários pendentes às 07:57:32Z. A interface desabilitou as ações enquanto
offline e reconectou automaticamente. O helper devolveu código 1 ao faltar o binário;
o SCM recusou a partida (retorno 8). A restauração do arquivo original, verificada por
hash, permitiu iniciar normalmente. A janela foi fechada e o programa desinstalado.

`finish-session.json` e `post-verification.json` confirmam sucesso, ausência do serviço,
processos, executáveis da aplicação, atalhos e porta de depuração, além dos hashes de
segurança intactos. Os desinstaladores usados diretamente podem permanecer nas pastas
de teste. A tentativa intermediária conserva uma falha do executor ao ler a saída do
subprocesso; os registros das fases mostram parada e remoção bem-sucedidas. A leitura
foi corrigida com um handle de processo próprio e validada com saídas 0 e 1 antes da
rodada final. Nenhuma correção do programa foi necessária para esses cenários.

## Migração do pacote publicado e limpeza — 2026-09-13

O instalador real da release v0.3.1 foi baixado e conferido pelo SHA-256 publicado
`198B1AAB9D788326F7588BA15CD2CFE97A5C46C0FF4B541AB635FD5A81665036`.
Seu defeito de nome do serviço foi reproduzido; o serviço antigo não iniciou e os
193 arquivos de dados continuaram idênticos. A atualização para o pacote atual
iniciou o serviço correto e respondeu ao Ping. Ambos os pacotes reportam 0.1.0.

Evidências: `target/beta/legacy-package-v0.3.1/upgrade-acceptance.json` e
`upgrade-completion.json`. O primeiro roteiro comparou incorretamente a UI instalada
com o executável avulso. A UI coincide com o conteúdo exato do instalador; os únicos
três bytes diferentes são o marcador UNK/NSS inserido pelo
[empacotador do Tauri](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle.rs).
A falha original foi preservada e apenas as verificações restantes foram retomadas.

A desinstalação revelou seis recursos antigos de CPU que não constavam mais do pacote.
O hook foi corrigido para excluir esses caminhos exatos e remover apenas diretórios
vazios, sem recursão. `legacy-cleanup-before.json` registra o defeito; o teste real
`legacy-cleanup-after.json` passou às 19:25:50Z: seis arquivos removidos, arquivo extra
preservado e driver PawnIO existente intacto. O arquivo extra de teste foi então retirado.

Novo instalador: SHA-256 `DFD6D122593DB92BD80033DEDADC8714E531143885BCA0470B692ADF3CCFD63C`,
build 19:24:15Z. Serviço, aplicação e atalhos de teste foram removidos, com histórico
de segurança intacto. Só o stub do desinstalador direto permanece. A alteração de
produção ficou no hook; o serviço é byte a byte o mesmo. A qualificação física D3
continua pendente e não foi substituída por este teste de instalação.
