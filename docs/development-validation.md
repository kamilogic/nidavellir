# Validação manual durante o desenvolvimento

Este fluxo usa os executáveis locais, sem instalar ou atualizar o pacote. A autorização
é uma exceção explícita para **uma Clean Run com duração Standard**, após revisar os incidentes. Ela não
apaga o histórico nem libera frequências/tensões já reprovadas.

Desde 16/09/2026, o caminho prioritário de aceitação do operador é sempre **Full Reset →
Clean Run**: redescobrir a placa sem reutilizar medições, fronteiras ou perfis positivos.
Não usar sucesso por Resume ou aprendizado persistente como prova desse início do zero.
Desde17/09/2026, por decisão explícita do operador, **Full Reset também apaga blacklist,
incidentes, condenações e arquivos de aprendizado**, depois de confirmar stock. Assim a
próxima busca não herda essas restrições. **Soft Reset** mantém as falhas conhecidas e
apaga os positivos. A autorização continua separada: resetar não concede uma nova run.

## Abrir pelo BAT e executar pela interface

O launcher mantido no projeto é `scripts/dev-launch.bat`. O arquivo
`C:\Users\leona\OneDrive\Desktop\dev.bat` o chama diretamente; não precisa colar outra versão.

1. Abra `dev.bat` e aceite o UAC. Ele compila/inicia o serviço em modo normal e abre a interface.
2. No programa, escolha **Soft Reset** ou **Full Reset**, com uma única confirmação na UI.
   A confirmação fecha imediatamente e a UI mostra o andamento; aguarde o resultado do Core.
3. Após o reset, a UI prepara **Clean Run**. Clique em **Forge GPU** para iniciar manualmente.

O BAT termina após abrir a interface: não pede S/N, não chama `authorize-validation.ps1`
e não usa `--development-validation`. Reset e Start são feitos pela interface. O UAC
continua necessário para abrir o Core elevado. Os limites normais de segurança e reboot
continuam valendo; a exceção de desenvolvimento abaixo é um fluxo separado para investigação.
Ao fechar uma sessão, encerre o serviço pelo Ctrl+C antes de abrir outro BAT.

## Alternativa por comandos separados

Abra um PowerShell normal na pasta do projeto:

```powershell
cd C:\Users\leona\dev\nidavellir
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/dev-service-admin.ps1 -Release -DevelopmentValidation
```

Aceite o UAC do Windows. O serviço abre em outro terminal, compila a versão release e
fica aguardando comandos. Aguarde terminar a inicialização; não abra outra instância.
Não use `scripts/dev.ps1` nesta validação: ele pode reiniciar o serviço ao salvar código.

No primeiro PowerShell, consulte a situação sem alterar nada:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/authorize-validation.ps1
```

Se houver recuperação pendente, exigência de reinicialização, perfil aplicado ou checkpoint,
resolva essa condição antes da autorização. Exporte o resultado anterior antes de limpar
suas medições. O comando informa a condição que impede continuar; ele não reconhece
incidentes automaticamente.

Faça **Full Reset antes de autorizar**. A ordem nesta validação é: exportar/revisar o
resultado anterior → Full Reset → autorização explícita → Clean Run. Se fez Full Reset
depois de autorizar, aquela permissão foi encerrada mesmo que a run ainda não tenha começado.
Nesse caso, encerre o serviço normalmente, abra uma sessão nova e só então autorize de novo,
após conferir o relatório. Não é necessário repetir o reset que já terminou com sucesso.

Para autorizar a tentativa planejada:

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/authorize-validation.ps1 -Authorize -Reason "Revisao dos tres incidentes; validar o fluxo completo preservando exclusoes historicas"
npm.cmd --prefix apps/ui run tauri:dev
```

O comando de autorização confirma o retorno a stock e grava um registro de auditoria;
**ele não inicia carga na GPU**. Na interface, confira o aviso **Development validation**,
selecione **Clean Run** e clique em **Forge GPU** quando estiver pronto. Clean Run usa os
tempos de Standard e é aceita por esta autorização. Não faça outro Full Reset depois dela.

## Durante e depois da run

- Mantenha o código, o serviço e a interface sem alterações durante a execução.
- A primeira nova queda encerra a permissão. Conclusão, Stop, Reset e fechamento do
  serviço também encerram a tentativa. Full Reset não renova a autorização.
- Esta exceção permite somente uma run fresca com tempos Standard (incluindo Clean Run); Resume, Long, outros testes
  manuais e Apply ficam indisponíveis nesta sessão. A etapa seguinte depende da revisão
  do resultado. Isso não declara nenhum perfil aprovado em uso real.
- Ao terminar, use **Review safety block → Export diagnostic report**. Guarde também os
  arquivos JSONL de observações indicados pelo exportador e o registro em
  `C:\ProgramData\Nidavellir\development-validations`. O relatório inclui o caminho da autorização.
- Envie o relatório, os logs e o que observou. Uma nova tentativa exige nova revisão e
  autorização explícita; o arquivo de auditoria nunca restaura uma permissão sozinho.
- Depois de exportar, feche a interface e use **Ctrl+C** no terminal do serviço para a
  saída controlada. A execução local não depende de a conversa com o Codex permanecer ativa.

Se um comando informar timeout/desconexão, consulte novamente **sem `-Authorize`** e
verifique o registro antes de outra ação. O cliente nunca repete uma autorização automaticamente.

## Evidência da implementação

### Diagnóstico de residência com carga variável

`nidavellir-service.exe diagnose-f2-loads --confirm --reason "motivo da investigação"`
executa um diagnóstico de desenvolvimento de um único ponto fixo, 1830 MHz @943 mV.
Reutiliza a transação protegida do diagnóstico existente: goldens stock, 120 s de aquecimento,
aplicação única, cinco fases DX11 de30 s e reset verificado. Os ritmos são100/75/50/25/100%:
nos intermediários, janelas de trabalho verificadas de100 ms alternam com pausas proporcionais.
Esses números são ritmos solicitados, não utilização de GPU garantida.

É um comando de investigação autorizado explicitamente, sem busca, perfil ou aprendizado positivo.
Usa as exclusões existentes e recusa recuperação pendente/checkpoint ativo na autorização.
O resultado global é deliberadamente não publicável; os resultados das fases e leituras de curva
ficam no journal `.point.jsonl` indicado na saída. Amostras incluem pausas e não constituem
prova de residência durante trabalho ativo. Não usar esse diagnóstico para aprovar perfis.

### Validação do fluxo manual

`target/beta/development-validation/` registra testes de software, três temas da interface,
quatro cenários do comando com pipes Windows isolados e a identidade do serviço release.
Esses testes não executaram autorização no hardware nem uma run real. O modo comum e o
serviço instalado continuam aplicando o bloqueio original de três incidentes / limite dois.
