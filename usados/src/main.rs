//! Usados — "Marcar como usado" no clique direito do Windows.
//!
//! Como o programa é chamado:
//!   Usados.exe                      → tela de configurações / instalação
//!   Usados.exe --config             → idem (item "Configurações..." do menu)
//!   Usados.exe --mover "arquivo"    → move para a pasta de usados configurada
//!   Usados.exe --mover-para "arq"   → pergunta a pasta e move
//!   Usados.exe --desinstalar        → remove tudo (usado pelo "Aplicativos instalados")
//!   Usados.exe arq1 arq2 ...        → igual a --mover (arrastar arquivos em cima do .exe)

// Sem janela preta de terminal
#![windows_subsystem = "windows"]

mod config;
mod fila;
mod mover;
mod ui;
mod util;

use std::path::PathBuf;

use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx};

fn main() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    }

    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let primeiro = args.first().and_then(|a| a.to_str()).map(|s| s.to_ascii_lowercase());
    let caminhos = |de: usize| -> Vec<PathBuf> { args.iter().skip(de).map(PathBuf::from).collect() };

    match primeiro.as_deref() {
        None | Some("--config") => ui::tela_configuracoes(),
        Some("--desinstalar") => {
            ui::desinstalar_interativo();
        }
        Some("--mover") => mover(fila::Modo::Padrao, caminhos(1)),
        Some("--mover-para") => mover(fila::Modo::Escolher, caminhos(1)),
        Some(_) => {
            // Arquivos arrastados em cima do .exe (ou pelo "Enviar para")
            let lista = caminhos(0);
            if lista.iter().all(|p| p.exists()) {
                mover(fila::Modo::Padrao, lista);
            } else {
                ui::tela_configuracoes();
            }
        }
    }

    config::ao_sair();
}

fn mover(modo: fila::Modo, caminhos: Vec<PathBuf>) {
    if caminhos.is_empty() {
        return;
    }
    if let Err(e) = fila::enfileirar(modo, &caminhos) {
        ui::erro("Não consegui registrar o pedido", &e.to_string());
        return;
    }
    fila::processar();
}
