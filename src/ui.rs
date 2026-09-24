//! Janelas do programa: configurações, mensagens e escolha de pasta.
//! Usa as caixas de diálogo nativas do Windows (TaskDialog e o seletor de pastas).

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, CoTaskMemFree};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::*;
use windows::Win32::UI::Shell::{
    FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog,
    IShellItem, SHCreateItemFromParsingName, SIGDN_FILESYSPATH,
};
use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow};
use windows::core::{BOOL, HSTRING, PCWSTR, w};

use crate::config::{self, Config, Organizacao};

const TITULO: &str = "Usados";

// IDs dos botões
const ID_PASTA: i32 = 101;
const ID_SALVAR: i32 = 102;
const ID_DESINSTALAR: i32 = 103;
const ID_SIM: i32 = 201;
const ID_NAO: i32 = 202;
const ID_RADIO_BASE: i32 = 300;

fn pcwstr(h: &HSTRING) -> PCWSTR {
    PCWSTR(h.as_ptr())
}

fn nulo_se_vazio(h: &HSTRING) -> PCWSTR {
    if h.is_empty() { PCWSTR::null() } else { pcwstr(h) }
}

/// Resultado de uma janela: botão clicado, opção (radio) marcada e caixinha marcada.
struct Resposta {
    botao: i32,
    radio: i32,
    caixa: bool,
}

#[derive(Default)]
struct Janela<'a> {
    instrucao: &'a str,
    conteudo: &'a str,
    rodape: &'a str,
    icone: Icone,
    botoes: Vec<(i32, String)>,
    radios: Vec<(i32, String)>,
    radio_padrao: i32,
    caixa: Option<(&'a str, bool)>,
    links: bool,
    botao_padrao: i32,
    comuns: TASKDIALOG_COMMON_BUTTON_FLAGS,
    largura: u32,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Icone {
    #[default]
    Programa,
    Aviso,
    Erro,
}

fn mostrar(j: &Janela) -> Resposta {
    let titulo = HSTRING::from(TITULO);
    let instrucao = HSTRING::from(j.instrucao);
    let conteudo = HSTRING::from(j.conteudo);
    let rodape = HSTRING::from(j.rodape);
    let caixa_txt = HSTRING::from(j.caixa.map(|c| c.0).unwrap_or(""));

    let textos_botoes: Vec<HSTRING> = j.botoes.iter().map(|(_, t)| HSTRING::from(t.as_str())).collect();
    let botoes: Vec<TASKDIALOG_BUTTON> = j
        .botoes
        .iter()
        .zip(&textos_botoes)
        .map(|((id, _), t)| TASKDIALOG_BUTTON { nButtonID: *id, pszButtonText: pcwstr(t) })
        .collect();
    let textos_radios: Vec<HSTRING> = j.radios.iter().map(|(_, t)| HSTRING::from(t.as_str())).collect();
    let radios: Vec<TASKDIALOG_BUTTON> = j
        .radios
        .iter()
        .zip(&textos_radios)
        .map(|((id, _), t)| TASKDIALOG_BUTTON { nButtonID: *id, pszButtonText: pcwstr(t) })
        .collect();

    let mut flags = TDF_ALLOW_DIALOG_CANCELLATION;
    if j.links {
        flags |= TDF_USE_COMMAND_LINKS;
    }
    if matches!(j.caixa, Some((_, true))) {
        flags |= TDF_VERIFICATION_FLAG_CHECKED;
    }

    let hinst = unsafe { GetModuleHandleW(None) }.unwrap_or_default();
    let mut cfg = TASKDIALOGCONFIG {
        cbSize: size_of::<TASKDIALOGCONFIG>() as u32,
        hInstance: hinst.into(),
        dwFlags: flags,
        dwCommonButtons: j.comuns,
        pszWindowTitle: pcwstr(&titulo),
        pszMainInstruction: nulo_se_vazio(&instrucao),
        pszContent: nulo_se_vazio(&conteudo),
        cButtons: botoes.len() as u32,
        pButtons: if botoes.is_empty() { std::ptr::null() } else { botoes.as_ptr() },
        nDefaultButton: j.botao_padrao,
        cRadioButtons: radios.len() as u32,
        pRadioButtons: if radios.is_empty() { std::ptr::null() } else { radios.as_ptr() },
        nDefaultRadioButton: j.radio_padrao,
        pszVerificationText: nulo_se_vazio(&caixa_txt),
        pszFooter: nulo_se_vazio(&rodape),
        cxWidth: j.largura,
        ..Default::default()
    };
    cfg.Anonymous1.pszMainIcon = match j.icone {
        Icone::Programa => PCWSTR(1 as _), // ícone nº 1 dentro do próprio .exe
        Icone::Aviso => TD_WARNING_ICON,
        Icone::Erro => TD_ERROR_ICON,
    };
    if !j.rodape.is_empty() {
        cfg.Anonymous2.pszFooterIcon = TD_INFORMATION_ICON;
    }

    let mut botao = 0i32;
    let mut radio = 0i32;
    let mut caixa = BOOL(0);
    let ok = unsafe {
        TaskDialogIndirect(&cfg, Some(&mut botao), Some(&mut radio), Some(&mut caixa))
    };
    if ok.is_err() {
        // Plano B (nunca deveria acontecer): caixa de mensagem simples.
        use windows::Win32::UI::WindowsAndMessaging::{MB_OK, MessageBoxW};
        let texto = HSTRING::from(format!("{}\n\n{}", j.instrucao, j.conteudo));
        unsafe { MessageBoxW(None, &texto, &titulo, MB_OK) };
        return Resposta { botao: 0, radio: j.radio_padrao, caixa: false };
    }
    Resposta { botao, radio, caixa: caixa.as_bool() }
}

// ---------------------------------------------------------------------------
// Mensagens simples
// ---------------------------------------------------------------------------

pub fn info(instrucao: &str, conteudo: &str) {
    mostrar(&Janela {
        instrucao,
        conteudo,
        icone: Icone::Programa,
        comuns: TDCBF_OK_BUTTON,
        ..Default::default()
    });
}

pub fn aviso(instrucao: &str, conteudo: &str) {
    mostrar(&Janela {
        instrucao,
        conteudo,
        icone: Icone::Aviso,
        comuns: TDCBF_OK_BUTTON,
        ..Default::default()
    });
}

pub fn erro(instrucao: &str, conteudo: &str) {
    mostrar(&Janela {
        instrucao,
        conteudo,
        icone: Icone::Erro,
        comuns: TDCBF_OK_BUTTON,
        ..Default::default()
    });
}

/// Pergunta com dois botões grandes. Retorna true se clicou no primeiro.
pub fn perguntar(instrucao: &str, conteudo: &str, sim: &str, nao: &str) -> bool {
    let r = mostrar(&Janela {
        instrucao,
        conteudo,
        icone: Icone::Programa,
        botoes: vec![(ID_SIM, sim.to_string()), (ID_NAO, nao.to_string())],
        links: true,
        botao_padrao: ID_SIM,
        ..Default::default()
    });
    r.botao == ID_SIM
}

// ---------------------------------------------------------------------------
// Seletor de pasta
// ---------------------------------------------------------------------------

pub fn escolher_pasta(titulo: &str, inicial: Option<&Path>) -> Option<PathBuf> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER).ok()?;
        let opcoes = dlg.GetOptions().ok()?;
        dlg.SetOptions(opcoes | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST).ok()?;
        let _ = dlg.SetTitle(&HSTRING::from(titulo));
        let _ = dlg.SetOkButtonLabel(w!("Usar esta pasta"));
        if let Some(p) = inicial.filter(|p| p.is_dir()) {
            if let Ok(item) = SHCreateItemFromParsingName::<_, _, IShellItem>(&HSTRING::from(p.as_os_str()), None) {
                let _ = dlg.SetFolder(&item);
            }
        }
        dlg.Show(None).ok()?; // cancelado → None
        let item = dlg.GetResult().ok()?;
        let texto = item.GetDisplayName(SIGDN_FILESYSPATH).ok()?;
        let caminho = texto.to_string().ok();
        CoTaskMemFree(Some(texto.0 as _));
        caminho.map(PathBuf::from)
    }
}

// ---------------------------------------------------------------------------
// Tela de configurações / instalação
// ---------------------------------------------------------------------------

fn texto_organizacao(o: Organizacao) -> &'static str {
    match o {
        Organizacao::Direto => "Tudo direto na pasta de destino",
        Organizacao::PorMes => "Separar em subpastas por mês   (ex.: 2026-09)",
        Organizacao::PorDia => "Separar em subpastas por dia   (ex.: 2026-09-24)",
        Organizacao::PorOrigem => "Separar pelo nome da pasta de onde o arquivo veio   (ex.: Casamento Ana)",
    }
}

/// Se a tela de configurações já estiver aberta, só traz ela para frente.
fn ja_aberta() -> bool {
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\Usados-Configuracoes"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // "#32770" é a classe das caixas de diálogo (não pega uma janela de pasta chamada "Usados").
            if let Ok(hwnd) = FindWindowW(w!("#32770"), w!("Usados")) {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
            return true;
        }
        // O mutex fica aberto até o programa fechar (de propósito).
        std::mem::forget(_mutex);
        false
    }
}

pub fn tela_configuracoes() {
    if ja_aberta() {
        return;
    }

    let mut cfg = config::carregar();
    let instalado = config::esta_instalado();
    let win11 = config::eh_windows_11();
    let classico_inicial = config::menu_classico_ativo();
    let mut classico = classico_inicial;

    loop {
        let pasta_txt = cfg
            .destino
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "(nenhuma escolhida ainda)".into());

        let mut conteudo = String::new();
        if !instalado {
            conteudo.push_str(
                "Este programa adiciona a opção “Marcar como usado” ao clique direito do Windows. \
                 Ela move os arquivos e pastas selecionados para a sua pasta de usados.\n\n",
            );
        }
        conteudo.push_str(&format!("Pasta de destino:\n{pasta_txt}\n\nComo organizar os arquivos lá dentro:"));

        let mut botoes = vec![(
            ID_PASTA,
            "Escolher a pasta de destino…\nOnde os arquivos usados vão ficar".to_string(),
        )];
        if instalado {
            botoes.push((ID_SALVAR, "Salvar\nAplica as mudanças no clique direito".into()));
            botoes.push((
                ID_DESINSTALAR,
                "Desinstalar\nRemove a opção do clique direito. Nenhum arquivo seu é apagado.".into(),
            ));
        } else {
            botoes.push((
                ID_SALVAR,
                "Instalar\nSó para este usuário, não precisa de administrador".into(),
            ));
        }

        let radios = [Organizacao::Direto, Organizacao::PorMes, Organizacao::PorDia, Organizacao::PorOrigem]
            .iter()
            .map(|o| (ID_RADIO_BASE + *o as i32, texto_organizacao(*o).to_string()))
            .collect();

        let rodape = if win11 {
            "No Windows 11, a opção fica em “Mostrar mais opções” (ou Shift + clique direito). \
             Marque a caixa acima para ela aparecer direto no clique direito."
        } else {
            ""
        };

        let r = mostrar(&Janela {
            instrucao: if instalado { "Marcar como usado — Configurações" } else { "Bem-vindo! Vamos configurar" },
            conteudo: &conteudo,
            rodape,
            icone: Icone::Programa,
            botoes,
            radios,
            radio_padrao: ID_RADIO_BASE + cfg.organizacao as i32,
            caixa: win11.then_some(("Mostrar direto no clique direito (menu clássico)", classico)),
            links: true,
            botao_padrao: if cfg.destino.is_some() { ID_SALVAR } else { ID_PASTA },
            comuns: TDCBF_CANCEL_BUTTON,
            largura: 300,
            ..Default::default()
        });

        cfg.organizacao = Organizacao::de_numero((r.radio - ID_RADIO_BASE).max(0) as u32);
        if win11 {
            classico = r.caixa;
        }

        match r.botao {
            ID_PASTA => {
                if let Some(p) = escolher_pasta("Escolha a pasta para os arquivos usados", cfg.destino.as_deref()) {
                    cfg.destino = Some(p);
                }
            }
            ID_SALVAR => {
                if salvar_e_instalar(&cfg, instalado, classico, classico_inicial) {
                    return;
                }
            }
            ID_DESINSTALAR => {
                if desinstalar_interativo() {
                    return;
                }
            }
            _ => return, // Cancelar / fechar
        }
    }
}

fn salvar_e_instalar(cfg: &Config, ja_instalado: bool, classico: bool, classico_inicial: bool) -> bool {
    let Some(destino) = cfg.destino.as_ref() else {
        aviso("Escolha a pasta de destino primeiro", "Clique em “Escolher a pasta de destino…”.");
        return false;
    };
    if let Err(e) = std::fs::create_dir_all(destino) {
        erro("Não consegui usar essa pasta", &format!("{}\n\n{e}", destino.display()));
        return false;
    }
    if let Err(e) = config::salvar(cfg) {
        erro("Não consegui salvar as configurações", &e.to_string());
        return false;
    }
    if let Err(e) = config::instalar(cfg) {
        erro("Não consegui instalar", &e);
        return false;
    }

    let mudou_menu = classico != classico_inicial;
    if mudou_menu {
        if let Err(e) = config::definir_menu_classico(classico) {
            aviso("Não consegui trocar o estilo do menu", &e.to_string());
        }
    }

    let como_usar = if !config::eh_windows_11() || classico {
        "Clique com o botão direito em qualquer arquivo ou pasta → “Marcar como usado”."
    } else {
        "Clique com o botão direito em qualquer arquivo ou pasta → “Mostrar mais opções” \
         (ou segure Shift ao clicar) → “Marcar como usado”."
    };

    let mut texto = format!(
        "{como_usar}\n\nDá para selecionar vários arquivos de uma vez. Eles vão para:\n{}",
        destino.display()
    );
    if !ja_instalado {
        texto.push_str(
            "\n\nO programa foi instalado no seu usuário, então o arquivo que você baixou já pode ser apagado. \
             Para mudar a pasta depois: clique direito → Marcar como usado → Configurações…",
        );
    }

    info(if ja_instalado { "Configurações salvas" } else { "Pronto, instalado!" }, &texto);

    if mudou_menu {
        perguntar_reiniciar_explorer();
    }
    true
}

fn perguntar_reiniciar_explorer() {
    if perguntar(
        "Reiniciar o Windows Explorer agora?",
        "A mudança no estilo do menu só aparece depois que o Explorer reinicia. \
         A barra de tarefas some por alguns segundos e as janelas de pastas abertas são fechadas \
         (nenhum arquivo é afetado).",
        "Reiniciar agora",
        "Depois\nVale a partir da próxima vez que o computador for reiniciado",
    ) {
        config::reiniciar_explorer();
    }
}

/// Retorna true se desinstalou.
pub fn desinstalar_interativo() -> bool {
    if !perguntar(
        "Desinstalar o “Marcar como usado”?",
        "A opção sai do clique direito e o programa é removido deste usuário.\n\n\
         A pasta de usados e tudo o que está dentro dela NÃO são apagados.",
        "Desinstalar",
        "Cancelar",
    ) {
        return false;
    }
    let r = config::desinstalar();
    info("Desinstalado", "A opção “Marcar como usado” foi removida do clique direito.");
    if r.desfez_menu_classico {
        perguntar_reiniciar_explorer();
    }
    true
}

// ---------------------------------------------------------------------------
// Resumo depois de mover
// ---------------------------------------------------------------------------

pub fn resumo_problemas(movidos: usize, problemas: &[(PathBuf, String)]) {
    let mut lista = String::new();
    for (p, motivo) in problemas.iter().take(12) {
        let nome = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
        lista.push_str(&format!("• {nome} — {motivo}\n"));
    }
    if problemas.len() > 12 {
        lista.push_str(&format!("• … e mais {}\n", problemas.len() - 12));
    }
    let instrucao = if movidos == 0 {
        "Nenhum item foi movido".to_string()
    } else if movidos == 1 {
        "1 item movido, mas alguns ficaram de fora".to_string()
    } else {
        format!("{movidos} itens movidos, mas alguns ficaram de fora")
    };
    mostrar(&Janela {
        instrucao: &instrucao,
        conteudo: lista.trim_end(),
        icone: Icone::Aviso,
        comuns: TDCBF_OK_BUTTON,
        largura: 300,
        ..Default::default()
    });
}
