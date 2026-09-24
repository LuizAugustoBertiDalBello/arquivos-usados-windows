//! Configurações (guardadas no Registro, por usuário) e instalação/desinstalação.
//!
//! Tudo fica em HKEY_CURRENT_USER, então não precisa de administrador e cada
//! pessoa que usa o computador tem a sua própria pasta de destino.

use std::io;
use std::path::{Path, PathBuf};

use winreg::RegKey;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};

use crate::util;

const CHAVE_APP: &str = r"Software\Usados";
const CHAVE_MENU: &str = r"Software\Classes\AllFilesystemObjects\shell\Usados";
const CHAVE_DESINSTALAR: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Usados";
/// Truque conhecido do Windows 11: com esta chave vazia, o clique direito volta a
/// mostrar o menu completo (clássico), sem precisar de "Mostrar mais opções".
const CHAVE_MENU_CLASSICO: &str = r"Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}";

pub const NOME_REGISTRO_CSV: &str = "_registro_usados.csv";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Organizacao {
    /// Tudo direto na pasta de destino
    Direto = 0,
    /// Destino\2026-09
    PorMes = 1,
    /// Destino\2026-09-24
    PorDia = 2,
    /// Destino\<nome da pasta de onde o arquivo veio>
    PorOrigem = 3,
}

impl Organizacao {
    pub fn de_numero(n: u32) -> Self {
        match n {
            1 => Self::PorMes,
            2 => Self::PorDia,
            3 => Self::PorOrigem,
            _ => Self::Direto,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub destino: Option<PathBuf>,
    pub organizacao: Organizacao,
    /// Grava o _registro_usados.csv (de onde veio cada arquivo). Padrão: sim.
    pub registrar: bool,
    /// Última pasta usada em "Mover para outra pasta..."
    pub ultima_outra_pasta: Option<PathBuf>,
}

fn hkcu() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}

pub fn carregar() -> Config {
    let chave = hkcu().open_subkey(CHAVE_APP).ok();
    let texto = |nome: &str| -> Option<String> {
        chave
            .as_ref()?
            .get_value::<String, _>(nome)
            .ok()
            .filter(|s| !s.trim().is_empty())
    };
    let numero = |nome: &str| -> Option<u32> { chave.as_ref()?.get_value::<u32, _>(nome).ok() };

    Config {
        destino: texto("Destino").map(PathBuf::from),
        organizacao: Organizacao::de_numero(numero("Organizacao").unwrap_or(0)),
        registrar: numero("RegistrarCsv").unwrap_or(1) != 0,
        ultima_outra_pasta: texto("UltimaOutraPasta").map(PathBuf::from),
    }
}

pub fn salvar(cfg: &Config) -> io::Result<()> {
    let (chave, _) = hkcu().create_subkey(CHAVE_APP)?;
    let destino = cfg
        .destino
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    chave.set_value("Destino", &destino)?;
    chave.set_value("Organizacao", &(cfg.organizacao as u32))?;
    chave.set_value("RegistrarCsv", &(cfg.registrar as u32))?;
    Ok(())
}

pub fn salvar_ultima_outra_pasta(p: &Path) {
    if let Ok((chave, _)) = hkcu().create_subkey(CHAVE_APP) {
        let _ = chave.set_value("UltimaOutraPasta", &p.to_string_lossy().into_owned());
    }
}

// ---------------------------------------------------------------------------
// Instalação
// ---------------------------------------------------------------------------

/// %LOCALAPPDATA%\Programs\Usados  (mesmo lugar que VS Code, Discord etc. usam)
pub fn pasta_instalacao() -> PathBuf {
    util::local_appdata().join("Programs").join("Usados")
}

pub fn exe_instalado() -> PathBuf {
    pasta_instalacao().join("Usados.exe")
}

/// %LOCALAPPDATA%\Usados  (fila de trabalho temporária)
pub fn pasta_dados() -> PathBuf {
    util::local_appdata().join("Usados")
}

pub fn esta_instalado() -> bool {
    hkcu().open_subkey(CHAVE_MENU).is_ok() && exe_instalado().exists()
}

pub fn eh_windows_11() -> bool {
    RegKey::predef(HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", KEY_READ)
        .and_then(|k| k.get_value::<String, _>("CurrentBuildNumber"))
        .ok()
        .and_then(|b| b.trim().parse::<u32>().ok())
        .is_some_and(|b| b >= 22000)
}

/// Copia o programa para a pasta de instalação e registra o menu de clique direito.
pub fn instalar(cfg: &Config) -> Result<(), String> {
    let destino_exe = exe_instalado();
    let atual = std::env::current_exe().map_err(|e| e.to_string())?;

    if !util::mesmo_caminho(&atual, &destino_exe) {
        std::fs::create_dir_all(pasta_instalacao())
            .map_err(|e| format!("Não consegui criar {}: {e}", pasta_instalacao().display()))?;
        std::fs::copy(&atual, &destino_exe).map_err(|e| {
            format!(
                "Não consegui copiar o programa para {}.\n\nSe ele estiver movendo arquivos agora, espere terminar e tente de novo.\n\nDetalhe: {e}",
                destino_exe.display()
            )
        })?;
    }

    registrar_menu(&destino_exe, cfg).map_err(|e| format!("Erro ao criar o menu: {e}"))?;
    registrar_desinstalador(&destino_exe).map_err(|e| format!("Erro ao registrar: {e}"))?;
    avisar_explorer();
    Ok(())
}

/// Cria (ou atualiza) o submenu:
///   Marcar como usado ▸  Mover para  D:\Usados
///                        Mover para outra pasta...
///                        ─────────────
///                        Configurações...
fn registrar_menu(exe: &Path, cfg: &Config) -> io::Result<()> {
    let _ = hkcu().delete_subkey_all(CHAVE_MENU);

    let exe_txt = exe.to_string_lossy();
    let icone = format!("{exe_txt},0");
    let (raiz, _) = hkcu().create_subkey(CHAVE_MENU)?;
    raiz.set_value("MUIVerb", &"Marcar como usado")?;
    raiz.set_value("Icon", &icone)?;
    raiz.set_value("SubCommands", &"")?;

    let destino = cfg
        .destino
        .as_ref()
        .map(|p| util::encurtar(&p.to_string_lossy(), 60))
        .unwrap_or_else(|| "(pasta não configurada)".into());
    // "&" no menu vira atalho de teclado; "&&" mostra um "&" de verdade.
    let destino = destino.replace('&', "&&");

    let itens: [(&str, String, &str, bool); 3] = [
        ("1_mover", format!("Mover para  {destino}"), "--mover", false),
        ("2_outra", "Mover para outra pasta...".into(), "--mover-para", false),
        ("3_config", "Configurações...".into(), "--config", true),
    ];

    for (chave, rotulo, argumento, separador_antes) in itens {
        let (verbo, _) = raiz.create_subkey(format!(r"shell\{chave}"))?;
        verbo.set_value("MUIVerb", &rotulo)?;
        // "Player" = funciona com qualquer quantidade de itens selecionados
        // (o padrão do Windows some com a opção acima de 15 itens).
        verbo.set_value("MultiSelectModel", &"Player")?;
        if separador_antes {
            verbo.set_value("CommandFlags", &0x20u32)?; // ECF_SEPARATORBEFORE
        } else {
            verbo.set_value("Icon", &icone)?;
        }
        let (comando, _) = verbo.create_subkey("command")?;
        let linha = if argumento == "--config" {
            format!("\"{exe_txt}\" --config")
        } else {
            format!("\"{exe_txt}\" {argumento} \"%1\"")
        };
        comando.set_value("", &linha)?;
    }
    Ok(())
}

/// Aparece em Configurações › Aplicativos › Aplicativos instalados, para desinstalar por lá.
fn registrar_desinstalador(exe: &Path) -> io::Result<()> {
    let exe_txt = exe.to_string_lossy().into_owned();
    let (k, _) = hkcu().create_subkey(CHAVE_DESINSTALAR)?;
    k.set_value("DisplayName", &"Usados (Marcar como usado)")?;
    k.set_value("DisplayVersion", &env!("CARGO_PKG_VERSION"))?;
    k.set_value("Publisher", &"Usados")?;
    k.set_value("DisplayIcon", &format!("{exe_txt},0"))?;
    k.set_value("InstallLocation", &pasta_instalacao().to_string_lossy().into_owned())?;
    k.set_value("UninstallString", &format!("\"{exe_txt}\" --desinstalar"))?;
    k.set_value("NoModify", &1u32)?;
    k.set_value("NoRepair", &1u32)?;
    let tamanho_kb = std::fs::metadata(exe).map(|m| (m.len() / 1024) as u32).unwrap_or(0);
    k.set_value("EstimatedSize", &tamanho_kb)?;
    Ok(())
}

pub struct ResultadoDesinstalacao {
    /// O menu clássico do Windows 11 foi desfeito (precisa reiniciar o Explorer).
    pub desfez_menu_classico: bool,
}

pub fn desinstalar() -> ResultadoDesinstalacao {
    let ativado_por_nos = hkcu()
        .open_subkey(CHAVE_APP)
        .and_then(|k| k.get_value::<u32, _>("MenuClassicoPeloUsados"))
        .unwrap_or(0)
        != 0;

    let _ = hkcu().delete_subkey_all(CHAVE_MENU);
    let _ = hkcu().delete_subkey_all(CHAVE_DESINSTALAR);
    let mut desfez = false;
    if ativado_por_nos && menu_classico_ativo() {
        desfez = definir_menu_classico(false).is_ok();
    }
    let _ = hkcu().delete_subkey_all(CHAVE_APP);
    let _ = std::fs::remove_dir_all(pasta_dados());

    // Apaga o programa instalado. Se for este mesmo .exe (que está aberto),
    // pede para o Windows apagar alguns segundos depois que ele fechar.
    let pasta = pasta_instalacao();
    let atual = std::env::current_exe().unwrap_or_default();
    if util::esta_dentro(&atual, &pasta) {
        *APAGAR_AO_SAIR.lock().unwrap_or_else(|e| e.into_inner()) = Some(pasta);
    } else {
        let _ = std::fs::remove_dir_all(&pasta);
    }

    avisar_explorer();
    ResultadoDesinstalacao { desfez_menu_classico: desfez }
}

static APAGAR_AO_SAIR: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// Chamado logo antes do programa fechar.
pub fn ao_sair() {
    let pendente = APAGAR_AO_SAIR.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(pasta) = pendente {
        apagar_depois(&pasta);
    }
}

/// Um .exe não consegue apagar a si mesmo enquanto está aberto. Então deixamos um
/// comando escondido tentando apagar a pasta a cada ~1 s (por até 30 s) depois que fechamos.
fn apagar_depois(pasta: &Path) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let p = pasta.display();
    let cmd = format!(
        "/d /c for /l %i in (1,1,30) do (ping -n 2 127.0.0.1 >nul & rmdir /s /q \"{p}\" 2>nul & if not exist \"{p}\" exit)"
    );
    let _ = std::process::Command::new("cmd.exe")
        .raw_arg(cmd)
        .current_dir(std::env::temp_dir())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

// ---------------------------------------------------------------------------
// Menu clássico do Windows 11
// ---------------------------------------------------------------------------

pub fn menu_classico_ativo() -> bool {
    hkcu()
        .open_subkey(format!(r"{CHAVE_MENU_CLASSICO}\InprocServer32"))
        .is_ok()
}

pub fn definir_menu_classico(ativar: bool) -> io::Result<()> {
    if ativar {
        let (k, _) = hkcu().create_subkey(format!(r"{CHAVE_MENU_CLASSICO}\InprocServer32"))?;
        k.set_value("", &"")?;
    } else {
        match hkcu().delete_subkey_all(CHAVE_MENU_CLASSICO) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    // Lembra se fomos nós que ativamos, para desfazer só nesse caso ao desinstalar.
    if let Ok((k, _)) = hkcu().create_subkey(CHAVE_APP) {
        let _ = k.set_value("MenuClassicoPeloUsados", &(ativar as u32));
    }
    Ok(())
}

/// Reinicia o Windows Explorer para a troca de menu valer na hora.
pub fn reiniciar_explorer() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("taskkill.exe")
        .args(["/f", "/im", "explorer.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
    std::thread::sleep(std::time::Duration::from_millis(1200));
    let windir = std::env::var_os("WINDIR").map(PathBuf::from).unwrap_or_else(|| r"C:\Windows".into());
    let _ = std::process::Command::new(windir.join("explorer.exe")).spawn();
}

fn avisar_explorer() {
    use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}
