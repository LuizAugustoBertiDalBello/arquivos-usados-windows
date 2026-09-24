# Usados: "Marcar como usado" no clique direito do Windows

Um programa pequeno (um único `.exe` de ~400 KB, feito em Rust) que adiciona ao clique direito do Windows a opção de **mover arquivos e pastas para uma pasta de "usados"**.

```
Clique direito em arquivos/pastas
└── Marcar como usado  ▸  Mover para  D:\Usados
                          Mover para outra pasta...
                          ──────────────
                          Configurações...
```

## Para quem vai usar

### Instalar
1. Dê dois cliques em `Usados.exe`.
   - Se aparecer "O Windows protegeu o computador", clique em **Mais informações → Executar assim mesmo**. O aviso aparece porque o programa não tem assinatura digital paga.
2. Clique em **Escolher a pasta de destino…** e escolha onde os arquivos usados vão ficar.
3. Escolha como organizar (tudo junto, por mês, por dia ou pelo nome da pasta de origem).
4. Clique em **Instalar**.

Não precisa de administrador. O programa se copia para `%LOCALAPPDATA%\Programs\Usados`, então o arquivo baixado pode ser apagado depois.

### Usar
Selecione um ou vários arquivos/pastas → clique direito → **Marcar como usado** → **Mover para …**

- **Windows 11:** a opção fica em **"Mostrar mais opções"** (ou segure **Shift** ao clicar com o botão direito). Para ela aparecer direto no primeiro menu, marque a caixa **"Mostrar direto no clique direito (menu clássico)"** nas Configurações. Isso volta o menu completo do Windows 10 para este usuário; dá para desmarcar quando quiser.
- Pode selecionar centenas de arquivos: tudo é movido **num lote só**, com a janela de progresso normal do Windows.
- **Nada é sobrescrito.** Se já existir um arquivo com o mesmo nome no destino, o novo vira `nome (2).mp4`.
- Arquivo aberto no Premiere/DaVinci/etc.? O Windows mostra o aviso de "arquivo em uso", com as opções de tentar de novo ou pular.
- Mover no **mesmo disco** é instantâneo. Para **outro disco** (ex.: HD externo), o Windows copia e apaga o original, o que pode demorar com vídeos grandes.
- **Mover para outra pasta...** pergunta o destino na hora (e lembra a última pasta escolhida).

### Registro: de onde veio cada arquivo
Na pasta de destino fica o arquivo `_registro_usados.csv` (abre no Excel), com data, nome, **caminho original** e caminho novo de cada item movido. Serve para achar de onde algo veio, se precisar devolver.

### Mudar a pasta ou a organização
Clique direito → **Marcar como usado → Configurações…** (ou abra o `Usados.exe` de novo).

### Vários usuários no mesmo computador
Tudo fica no perfil de cada usuário do Windows (registro `HKEY_CURRENT_USER`). Cada pessoa instala no próprio usuário e escolhe a própria pasta.

### Desinstalar
**Configurações do Windows → Aplicativos → Aplicativos instalados → "Usados (Marcar como usado)" → Desinstalar**, ou pelo botão **Desinstalar** nas Configurações do programa.
A pasta de usados e os arquivos dentro dela **não são apagados**.

---

## Para quem vai mexer no código

### Compilar no Windows
1. Instale o Rust: https://rustup.rs (ele oferece instalar o "Visual Studio Build Tools", aceite).
2. Nesta pasta:
   ```
   cargo build --release
   ```
3. O programa sai em `target\release\usados.exe` (renomeie para `Usados.exe` se quiser).

### Compilar no Linux (cross-compile)
```
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
cargo build --release --target x86_64-pc-windows-gnu
```

### Organização do código
| Arquivo | O que faz |
|---|---|
| `src/main.rs` | Lê a linha de comando e decide o que fazer |
| `src/ui.rs` | Janelas: configurações, mensagens, seletor de pasta (diálogos nativos do Windows) |
| `src/config.rs` | Configurações no registro, instalar/desinstalar, menu do clique direito, menu clássico do Win 11 |
| `src/fila.rs` | Junta vários arquivos selecionados num lote só (explicação abaixo) |
| `src/mover.rs` | Move de verdade (`IFileOperation`, o mesmo mecanismo do Explorer) e grava o CSV |
| `src/util.rs` | Datas, nomes sem repetição, comparação de caminhos |
| `res/` | Ícone, manifesto (visual moderno, sem admin) e informações de versão |

### Linha de comando
```
Usados.exe                     tela de configurações / instalação
Usados.exe --config            idem
Usados.exe --mover "arquivo"   move para a pasta configurada
Usados.exe --mover-para "arq"  pergunta a pasta e move
Usados.exe --desinstalar       desinstala
Usados.exe arq1 arq2 ...       igual a --mover (arrastar arquivos em cima do .exe)
```

### Por que existe uma "fila"
Quando você seleciona 50 arquivos, o Windows abre o programa 50 vezes, uma por arquivo. Cada cópia anota o seu arquivo em `%LOCALAPPDATA%\Usados\fila` e tenta pegar um mutex. Só uma consegue e vira a "trabalhadora": ela espera os pedidos pararem de chegar (~0,5 s) e move tudo de uma vez. As outras cópias fecham na hora. O menu usa `MultiSelectModel=Player` para funcionar com mais de 15 itens selecionados, que é o limite padrão do Windows.

### O que fica no registro (tudo em HKEY_CURRENT_USER)
- `Software\Usados`: configurações (`Destino`, `Organizacao` 0–3, `RegistrarCsv` 0/1)
- `Software\Classes\AllFilesystemObjects\shell\Usados`: o menu do clique direito
- `Software\Microsoft\Windows\CurrentVersion\Uninstall\Usados`: entrada em "Aplicativos instalados"
- `Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}`: só se o menu clássico do Windows 11 for ativado

Para não gravar o CSV, defina `RegistrarCsv` = 0 em `HKEY_CURRENT_USER\Software\Usados`.
