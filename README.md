# Skill Gap

Otimizador de PC para Windows 10/11, feito com **Tauri v2 + React + TypeScript**. Ele mexe em ajustes do sistema; nunca no processo do jogo.

## Rodando no Windows

**Pré-requisitos (uma vez só):**

1. [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/), com a carga de trabalho "Desenvolvimento para desktop com C++".
2. Rust via [rustup](https://rustup.rs) (toolchain `stable-x86_64-pc-windows-msvc`, que é o padrão).
3. Node.js 20 ou mais recente.
4. WebView2: já vem no Windows 11 e nos Windows 10 atualizados.

**Comandos:**

```bash
npm install
npm run tauri dev     # abre o app com hot reload
npm run tauri build   # gera o instalador em src-tauri/target/release/bundle/nsis
```

A primeira compilação do Rust demora alguns minutos. As seguintes são incrementais.

> O app roda como usuário comum. Ações que precisam de administrador (HKLM, serviços, `sfc`/`DISM`, ponto de restauração…) abrem o aviso do UAC na hora. Em `npm run tauri dev` funciona igual: o auxiliar é o próprio executável de debug reaberto com `runas`.

## Estrutura

```
src/                      Front (React)
  styles/theme.css        Tokens de cor dos modos claro/escuro (variáveis CSS)
  styles/app.css          Componentes visuais
  hooks/theme.tsx         Tema: segue o Windows, salva a escolha manual, sincroniza a barra de título
  hooks/useResource.ts    Carregamento assíncrono com recarga e atualização otimista
  lib/api.ts              Wrappers tipados do invoke() e tipos espelhados do Rust
  components/             Layout (sidebar + fundo) e componentes base (Switch, PageHeader...)
  pages/                  Uma tela por arquivo

src-tauri/                Backend (Rust)
  src/lib.rs              Comandos expostos ao front
  src/error.rs            AppError → JSON { kind, message }
  src/startup/            Programas de inicialização (registro + pastas Startup)
  src/power/              Plano de energia (powercfg)
  src/gaming/             Modo Jogo: mod.rs (regras, corrigir/desfazer), watcher.rs (jogos monitorados),
                          windows.rs (monitores, mouse, registro, lista de processos)
  src/tweaks/             Motor de ajustes: ops.rs (operações + trait Sys), catalog.rs (ajustes e perfis),
                          mod.rs (diário, aplicar/desfazer), windows.rs (registro, serviços), fake.rs (sistema falso p/ testes)
  src/system/             Info do PC, ponto de restauração, reiniciar Explorer
  src/monitor/            Contadores PDH (nomes em inglês, funcionam em qualquer idioma do Windows)
  src/drivers/            Inventário via CIM, links oficiais (lista fixa), backup com pnputil
  src/fixes/              Catálogo de correções e execução passo a passo
  src/power/plans.rs      Planos de energia do Skill Gap
  src/bootperf/           Tempo de boot e quem atrasa (eventos 100/101/103)
  src/apps/               Apps de fábrica: lista curta e conservadora, reinstalação pela Store
  src/cleanup/            Limpeza: fsops.rs (varredura/remoção segura), windows.rs (caminhos, Lixeira, espaço livre)
  capabilities/           Permissões mínimas da janela
  tauri.conf.json         Janela, CSP e instalador
```

## O que já funciona

| Tela | Status |
|---|---|
| Todos os ajustes | ✅ 20 ajustes reversíveis com busca, categorias, favoritos e “salvar como perfil”; desfazer persistente (sobrevive a fechar o app e a queda de energia) |
| Perfis | ✅ 6 perfis prontos + perfis salvos; “Perfil para o seu PC” detecta notebook, RAM e SSD/HD; ponto de restauração opcional antes de aplicar |
| Início → Otimizar agora | ✅ Aplica o perfil recomendado, com progresso, “Criar ponto agora” e “Desfazer tudo” |
| Monitor | ✅ CPU, memória, GPU (3D + VRAM), disco e rede ao vivo pelos contadores de desempenho do Windows (PDH), 60 s de histórico com hover |
| Drivers | ✅ Inventário de drivers de fabricante, dispositivos com erro, links só para sites oficiais, backup com pnputil |
| Correções | ✅ DNS, reset de rede, reparar Windows Update, sfc, DISM, relógio e Explorer — uma por vez, com log de cada passo |
| Energia | ✅ Planos próprios “PC de mesa” e “Notebook” (cópias de planos do Windows com GUID fixo), indicação pelo tipo de PC e “Desfazer” que volta ao plano anterior e apaga as cópias |
| Mouse | ✅ Aceleração e velocidade 1:1 (motor de ajustes) + teste de taxa de resposta em Hz dentro do app |
| Inicialização | ✅ Lista HKCU/HKLM Run, Run 32 bits e pastas Startup; ativa e desativa itens; tempo de boot/login e atraso de cada programa (log Diagnostics-Performance); apps de fábrica |
| Desempenho → Plano de energia | ✅ Lê o plano ativo e troca entre Economia, Balanceado e Alto desempenho |
| Início | ✅ Resumo e recomendações calculados com dados reais |
| Limpeza | ✅ Mede em paralelo, limpa com progresso ao vivo, pula arquivos em uso e mostra o espaço realmente liberado |
| Modo Jogo → Diagnóstico | ✅ (mouse, Game Bar, HAGS e Modo de Jogo agora usam o motor de ajustes, com desfazer salvo em disco) Taxa do monitor, aceleração do mouse, gravação da Game Bar, HAGS e Modo de Jogo, com corrigir/desfazer; limite de FPS calculado pelo monitor |
| Modo Jogo automático | ✅ Detecta jogo/programa da lista e, durante a partida: prioridade alta via IFEO, apps em segundo plano em “abaixo do normal”, WSearch/SysMain pausados, plano Alto desempenho. Tudo volta ao fechar, inclusive após queda de energia |
| Efeitos visuais | 🎨 Só o visual |

## Decisões de projeto (e por quê)

- **Desativar sem apagar.** Itens de inicialização são desativados pelo marcador `Explorer\StartupApproved`, o mesmo do Gerenciador de Tarefas. A entrada original em `Run` nunca é apagada, então não precisa de backup e o "desfazer" é só reativar.
- **`spawn_blocking` nos comandos.** Registro e `powercfg` bloqueiam a thread. Rodar fora do runtime async evita travar a interface.
- **Erro tipado.** O front recebe `{ kind, message }` e trata `requiresAdmin` de forma específica, em vez de mostrar "os error 5".
- **Limpeza segura.** Nunca segue link simbólico/junction (um junction no %TEMP% apontando pra `C:\Users` seria uma catástrofe com um "apaga recursivo" ingênuo), nunca apaga a pasta raiz, pula arquivo em uso em vez de abortar e preserva arquivos com menos de 24 h no TEMP.
- **Duas medidas de espaço liberado.** A soma do tamanho dos arquivos apagados e a diferença de espaço livre no disco (`GetDiskFreeSpaceExW`). Elas diferem por causa de clusters, compressão NTFS, hard links e outros programas escrevendo ao mesmo tempo.
- **Threads na varredura, sequencial na limpeza.** Varrer é esperar disco, então 5 threads (`thread::scope`) adiantam. Apagar em paralelo no mesmo disco não ganha nada e bagunçaria o progresso.
- **Lixeira pela API do shell** (`SHEmptyRecycleBinW`), não apagando `$Recycle.Bin` na mão: ela tem uma subpasta por usuário em cada unidade, com metadados próprios.
- **Desfazer guarda o valor original, não um "padrão".** Antes de corrigir, o app tira uma foto do valor atual e guarda no estado gerenciado do Tauri (`app.manage` + `Mutex`). Corrigir duas vezes não sobrescreve a foto original. Hoje essa foto vive na memória (some ao fechar o app); persistir em disco é o item "Histórico" dos próximos passos.
- **Taxa do monitor sem trocar resolução.** Só sobe a frequência dentro da resolução atual, e testa a mudança com `CDS_TEST` antes de aplicar de verdade.
- **Limite de FPS pela fórmula do Reflex** (`3600·Hz / (3600 + Hz)`): 144 Hz → 138 FPS, 240 Hz → 225 FPS.
- **Prioridade do jogo sem abrir o processo.** `SetPriorityClass` exigiria abrir o processo do jogo (o que anticheats vigiam). Em vez disso, o app grava `Image File Execution Options\<jogo.exe>\PerfOptions\CpuPriorityClass = 3` e o próprio Windows aplica a prioridade alta quando o jogo inicia. O valor original é anotado e restaurado ao tirar o jogo da lista ou desligar o Modo Jogo.
- **Lista de protegidos.** Sistema (svchost, csrss, dwm...), áudio (audiodg), antivírus (MsMpEng) e anticheats (vgc, EasyAntiCheat, BEService, FACEIT) nunca entram em lista nenhuma: a validação recusa.
- **Respeita a escolha do usuário.** Só baixa a prioridade de quem está em "normal" e só restaura quem ainda está como nós deixamos. O Windows reaproveita PIDs, então a restauração confere PID + nome.
- **Sessão em disco.** `modo-jogo-sessao.json` é gravado antes de cada mudança; se existir ao abrir o app, `recover()` desfaz o que ficou pendente.
- **Detecção de jogo sem abrir o processo.** A lista vem de um snapshot do Toolhelp (`CreateToolhelp32Snapshot`), que só lê a tabela de processos. Abrir handle no processo do jogo (`OpenProcess`) é o que cheats fazem e o que anticheats vigiam, então o app nunca faz isso.
- **Thread com "sleep interrompível".** O monitor acorda a cada 3 s com `recv_timeout` num canal; salvar a configuração manda `Wake` (verifica na hora) e fechar o app manda `Stop`. O estado é compartilhado com `Arc`, porque a thread vive mais que qualquer referência emprestada.
- **Máquina de estados pura** (`decide`): Nada / Iniciar / Trocar de jogo / Parar. É testada sem tocar no sistema; só `tick()` aplica o resultado.
- **Restaura até se o app fechar no meio do jogo** (`RunEvent::Exit`), e guarda o GUID do plano anterior, então funciona com planos personalizados.
- **Config salva de forma atômica** (grava num `.tmp` e renomeia) em `%APPDATA%\app.skillgap.desktop\modo-jogo.json`. JSON corrompido = config vazia, nunca app que não abre.
- **Ajustes declarativos.** Cada ajuste é uma lista de operações ("este valor do registro deve ser X"). O motor sabe ler, fotografar, aplicar e restaurar qualquer operação, então adicionar um ajuste novo é adicionar uma entrada no catálogo, sem código novo.
- **"Anota antes de mexer".** A foto dos valores originais é gravada em `%APPDATA%\app.skillgap.desktop\tweaks.json` ANTES de aplicar. Se uma operação falhar no meio, as anteriores são desfeitas na hora. Aplicar duas vezes nunca sobrescreve a foto original.
- **Sem foto, sem desligar.** Se um ajuste já estava ativo antes do Skill Gap, a chave fica travada: o app não tem um "antes" pra voltar e não inventa um.
- **Honestidade no catálogo.** Ajustes de efeito pequeno (SysMain, compactação de memória, Timer Resolution) têm selo e explicação, e ficam fora dos perfis. Placebos conhecidos (limpar RAM, NetworkThrottlingIndex) não entram.
- **Testes com sistema falso.** O motor fala com o Windows através do trait `Sys`; nos testes, um `FakeSys` em memória simula registro, serviços, plano de energia, mouse e até falhas e falta de admin.
- **Contadores em inglês.** `PdhAddEnglishCounterW` em vez de `PdhAddCounterW`: os nomes normais são traduzidos ("\Processador(...)" no Windows em português) e quebrariam o monitor fora do inglês.
- **Monitor pausa quando ninguém olha.** A coleta para quando a janela está minimizada.
- **App não baixa driver.** Só abre URLs de uma lista fixa (sites oficiais e `ms-settings:windowsupdate`); o backend recusa qualquer outra.
- **Correções em sequência e uma por vez.** Se um passo falha, os seguintes são pulados, exceto "ligar serviço": um reparo que parou o Windows Update sempre o religa.
- **Planos de energia com GUID fixo.** São cópias (`powercfg -duplicatescheme`) com GUID próprio: dá pra saber se já existem, atualizar sem duplicar e apagar ao desfazer. Opções que o hardware não tem (ex.: core parking) são puladas e listadas, sem falhar o plano inteiro.
- **Apps de fábrica conservador.** Lista fixa no código; Xbox/Game Bar, Store, Fotos, Calculadora, Bloco de Notas, Terminal e Assistência Rápida nunca entram. O backend remove pelo nome da lista estática (nada digitado pelo front vai pro PowerShell) e cada app removido tem o link da Store pra reinstalar.
- **Teste de Hz do mouse sem driver.** Conta os relatórios do mouse com `pointerrawupdate` (ou eventos agrupados do `pointermove`) dentro do próprio WebView. Nada de driver de terceiros pra "turbinar" a taxa.
- **Lógica pura separada da chamada ao Windows.** Interpretar a saída do `powercfg` e os bytes do `StartupApproved` são funções puras com testes (`cargo test`); só a camada `windows.rs` toca no sistema.
- **Admin sob demanda (processo auxiliar elevado).** O app abre como usuário comum (asInvoker). Quando uma ação esbarra em "acesso negado", o app reabre o próprio `.exe` com `runas --elevated-helper` (é aí que aparece o UAC) e conversa com ele por um pipe nomeado. Um UAC por sessão; o auxiliar fecha junto com o app. Detalhes em `src-tauri/src/elevation/`:
  - **Lista fechada, não "executa o que mandar".** Cada pedido é uma operação tipada (`SetDword`, `RunFix{id}`, `CleanCategory`…) e passa por `elevation::check` antes de rodar: valor HKLM só se algum ajuste do catálogo escreve aquele caminho+nome; IFEO só `<jogo>.exe\PerfOptions\CpuPriorityClass` (nunca `Debugger`, nunca anticheat); serviço só da lista do Modo Jogo/Correções. O UAC **não é fronteira de segurança** (palavras da própria Microsoft): o que protege é essa política.
  - **Pipe protegido.** Nome com 128 bits aleatórios, `FILE_FLAG_FIRST_PIPE_INSTANCE` (ninguém "pré-cria" o pipe), `PIPE_REJECT_REMOTE_CLIENTS`, DACL só pro usuário do app + SYSTEM/Administradores, e os dois lados conferem o PID do outro (`GetNamedPipeServer/ClientProcessId`).
  - **Segundo plano nunca pede UAC.** O Modo Jogo (pausar SysMain/WSearch no meio da partida) só usa o auxiliar se ele já estiver aberto; senão pula esse item. Abrir o app também não pede nada: o sync das prioridades IFEO acontece quando você mexe na lista de jogos.
  - **Trade-off conhecido:** depois de liberado, o auxiliar fica aberto até o app fechar. É mais cômodo que um UAC por clique, mas é um processo elevado vivo; por isso a política é estreita e o chip na barra lateral mostra quando ele está ativo.
- **Fontes empacotadas e CSP restrita.** Nada é carregado da internet; a CSP de produção só permite `'self'` e o IPC do Tauri.
- **`MemoryRouter`.** Num app desktop não há barra de endereço, então a rota fica só em memória.

## Testes

```bash
cd src-tauri
cargo test      # 74 testes (inclui a política do auxiliar elevado): motor de ajustes, perfis, limpeza, Modo Jogo, monitor, drivers, correções, planos de energia, log de boot, apps de fábrica
```

## Próximos passos

As quatro fases planejadas estão prontas. Ideias pra depois:

1. **Cache do Windows Update na Limpeza** usando o mesmo fluxo da correção "Reparar o Windows Update" (parar serviço → limpar → religar).
2. **Ponto de restauração automático** antes do "Desfazer tudo", não só antes de aplicar.
3. **VSync do driver** via NVAPI/ADLX (hoje é orientação manual).
4. **Fechar o auxiliar elevado após X minutos ocioso** (hoje ele vive até o app fechar).
5. **Atualizações automáticas** do app (plugin `updater` do Tauri) e **tour no primeiro uso**.
6. **Assinatura de código** do executável antes de distribuir (reduz falso positivo de antivírus e o aviso do SmartScreen).

O ícone foi gerado a partir de `app-icon.png`. Para trocar, substitua a imagem e rode `npx tauri icon app-icon.png -o src-tauri/icons`.
