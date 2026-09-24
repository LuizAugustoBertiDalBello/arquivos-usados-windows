//! A parte que move de verdade, usando o mesmo mecanismo do Explorer (IFileOperation):
//! janela de progresso do Windows, aviso de "arquivo em uso", funciona entre discos, etc.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};

use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::UI::Shell::{
    FOF_ALLOWUNDO, FOF_NOCONFIRMMKDIR, FOFX_ADDUNDORECORD, FileOperation, IFileOperation,
    IFileOperationProgressSink, IShellItem, SHCreateItemFromParsingName,
};
use windows::Win32::Foundation::E_NOTIMPL;
use windows::Win32::Storage::FileSystem::{MOVEFILE_COPY_ALLOWED, MOVEFILE_WRITE_THROUGH, MoveFileExW};
use windows::core::HSTRING;

use crate::config::{self, Organizacao};
use crate::fila::{Modo, Pedido};
use crate::ui;
use crate::util::{self, Agora};

pub fn executar_lote(lote: Vec<Pedido>) {
    let cfg = config::carregar();

    // Separa por tipo de pedido e tira repetidos
    let mut vistos = HashSet::new();
    let mut padrao = Vec::new();
    let mut escolher = Vec::new();
    for p in lote {
        if vistos.insert((p.modo, util::chave_caminho(&p.caminho))) {
            match p.modo {
                Modo::Padrao => padrao.push(p.caminho),
                Modo::Escolher => escolher.push(p.caminho),
            }
        }
    }

    padrao.sort();
    escolher.sort();

    if !padrao.is_empty() {
        match &cfg.destino {
            Some(destino) => mover_para(&padrao, destino, cfg.organizacao, cfg.registrar),
            None => {
                if ui::perguntar(
                    "A pasta de usados ainda não foi escolhida",
                    "Abra as configurações e escolha para onde os arquivos devem ir.",
                    "Abrir configurações",
                    "Cancelar",
                ) {
                    ui::tela_configuracoes();
                }
            }
        }
    }

    if !escolher.is_empty() {
        let titulo = if escolher.len() == 1 {
            let nome = escolher[0].file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            format!("Mover “{nome}” para…")
        } else {
            format!("Mover {} itens para…", escolher.len())
        };
        let inicial = cfg.ultima_outra_pasta.as_deref().or(cfg.destino.as_deref());
        if let Some(pasta) = ui::escolher_pasta(&titulo, inicial) {
            config::salvar_ultima_outra_pasta(&pasta);
            // Aqui a pessoa escolheu a pasta exata, então não cria subpastas.
            mover_para(&escolher, &pasta, Organizacao::Direto, cfg.registrar);
        }
    }
}

/// Nome da subpasta no modo "pela pasta de origem".
fn nome_da_origem(item: &Path) -> String {
    let nome = match item.parent() {
        Some(pai) => match pai.file_name() {
            Some(n) => n.to_string_lossy().into_owned(),
            // Arquivo solto na raiz de um disco, ex.: D:\video.mp4 → "Disco D"
            None => format!("Disco {}", pai.to_string_lossy()),
        },
        None => String::new(),
    };
    // Tira caracteres que o Windows não aceita em nome de pasta
    let limpo: String = nome
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if limpo.is_empty() { "Outros".into() } else { limpo }
}

struct Movido {
    origem: PathBuf,
    destino: PathBuf,
    eh_pasta: bool,
    /// Erro já conhecido antes de conferir (ex.: o Windows recusou o item).
    erro: Option<String>,
}

pub fn mover_para(itens: &[PathBuf], raiz: &Path, org: Organizacao, registrar: bool) {
    if let Err(e) = std::fs::create_dir_all(raiz) {
        ui::erro(
            "Não consegui acessar a pasta de destino",
            &format!(
                "{}\n\nSe ela fica num HD externo ou na rede, confira se está conectado.\n\nDetalhe: {e}",
                raiz.display()
            ),
        );
        return;
    }

    let agora = util::agora();
    let mut problemas: Vec<(PathBuf, String)> = Vec::new();
    let mut planejados: Vec<Movido> = Vec::new();
    let mut nomes_reservados = HashSet::new();

    // 1) Decide para onde vai cada item (e com que nome)
    for item in itens {
        let Ok(meta) = std::fs::symlink_metadata(item) else {
            problemas.push((item.clone(), "não foi encontrado (já foi movido ou apagado?)".into()));
            continue;
        };
        let eh_pasta = meta.is_dir();

        if util::esta_dentro(item, raiz) {
            problemas.push((item.clone(), "já está na pasta de destino".into()));
            continue;
        }
        if util::esta_dentro(raiz, item) {
            problemas.push((item.clone(), "a pasta de destino fica dentro dele".into()));
            continue;
        }
        let Some(nome) = item.file_name() else {
            problemas.push((item.clone(), "não dá para mover um disco inteiro".into()));
            continue;
        };

        let pasta_alvo = match org {
            Organizacao::Direto => raiz.to_path_buf(),
            Organizacao::PorMes => raiz.join(agora.ano_mes()),
            Organizacao::PorDia => raiz.join(agora.ano_mes_dia()),
            Organizacao::PorOrigem => raiz.join(nome_da_origem(item)),
        };
        if let Err(e) = std::fs::create_dir_all(&pasta_alvo) {
            problemas.push((item.clone(), format!("não consegui criar {}: {e}", pasta_alvo.display())));
            continue;
        }

        // Se já existir um arquivo com o mesmo nome lá, usa "nome (2).ext" — nunca sobrescreve.
        let novo_nome = util::nome_livre(&pasta_alvo, nome, eh_pasta, &mut nomes_reservados);
        planejados.push(Movido {
            origem: item.clone(),
            destino: pasta_alvo.join(&novo_nome),
            eh_pasta,
            erro: None,
        });
    }

    // 2) Move: pelo Windows (janela de progresso, aviso de arquivo em uso...)
    //    ou, se isso não estiver disponível, pelo método simples.
    if !planejados.is_empty() && !mover_pelo_windows(&mut planejados) {
        for m in planejados.iter_mut() {
            if let Err(e) = mover_simples(&m.origem, &m.destino, m.eh_pasta) {
                m.erro = Some(e.to_string());
            }
        }
    }

    let mut movidos = Vec::new();
    for m in planejados {
        let chegou = m.destino.exists();
        let saiu = std::fs::symlink_metadata(&m.origem).is_err();
        if chegou && saiu {
            movidos.push(m);
        } else if chegou {
            problemas.push((m.origem, "foi movido só em parte (cancelado ou algum arquivo em uso)".into()));
        } else {
            let motivo = m.erro.unwrap_or_else(|| "não foi movido (cancelado, em uso ou sem permissão)".into());
            problemas.push((m.origem, motivo));
        }
    }

    if registrar && !movidos.is_empty() {
        registrar_csv(raiz, &movidos, &agora);
    }
    if !problemas.is_empty() {
        ui::resumo_problemas(movidos.len(), &problemas);
    }
}

/// Move usando o próprio Windows (IFileOperation), igual ao Explorer.
/// Retorna false se esse recurso não estiver disponível (aí usamos o método simples).
fn mover_pelo_windows(planejados: &mut [Movido]) -> bool {
    let op: IFileOperation = match unsafe { CoCreateInstance(&FileOperation, None, CLSCTX_ALL) } {
        Ok(op) => op,
        Err(_) => return false,
    };
    unsafe {
        let _ = op.SetOperationFlags(FOF_NOCONFIRMMKDIR | FOF_ALLOWUNDO | FOFX_ADDUNDORECORD);
    }

    let mut pastas: HashMap<PathBuf, IShellItem> = HashMap::new();
    let mut algum = false;
    for m in planejados.iter_mut() {
        let (Some(pasta), Some(nome)) = (m.destino.parent(), m.destino.file_name()) else {
            continue;
        };
        let si_pasta = match pastas.get(pasta) {
            Some(si) => si.clone(),
            None => match item_shell(pasta) {
                Ok(si) => {
                    pastas.insert(pasta.to_path_buf(), si.clone());
                    si
                }
                Err(e) => {
                    m.erro = Some(format!("erro na pasta de destino: {}", e.message()));
                    continue;
                }
            },
        };
        let si_item = match item_shell(&m.origem) {
            Ok(si) => si,
            Err(e) => {
                m.erro = Some(e.message());
                continue;
            }
        };
        let r = unsafe {
            op.MoveItem(&si_item, &si_pasta, &HSTRING::from(nome), None::<&IFileOperationProgressSink>)
        };
        match r {
            Ok(()) => algum = true,
            Err(e) if e.code() == E_NOTIMPL => return false,
            Err(e) => m.erro = Some(e.message()),
        }
    }

    if algum {
        // Aqui o Windows mostra a janela de progresso (se demorar) e trata arquivos em uso.
        // Se a pessoa cancelar, retorna erro — por isso conferimos item por item depois.
        let _ = unsafe { op.PerformOperations() };
    }
    true
}

/// Método simples (reserva): renomeia; se for para outro disco, copia e apaga o original.
fn mover_simples(origem: &Path, destino: &Path, eh_pasta: bool) -> std::io::Result<()> {
    let de = HSTRING::from(origem.as_os_str());
    let para = HSTRING::from(destino.as_os_str());
    // Sem MOVEFILE_REPLACE_EXISTING: nunca sobrescreve nada.
    let r = unsafe { MoveFileExW(&de, &para, MOVEFILE_COPY_ALLOWED | MOVEFILE_WRITE_THROUGH) };
    match r {
        Ok(()) => Ok(()),
        Err(e) if eh_pasta && (e.code().0 & 0xFFFF) == 17 => {
            // ERROR_NOT_SAME_DEVICE: pastas não podem ser "renomeadas" para outro disco,
            // então copia tudo e depois apaga o original.
            copiar_pasta(origem, destino)?;
            std::fs::remove_dir_all(origem)
        }
        Err(e) => Err(std::io::Error::from_raw_os_error(e.code().0 & 0xFFFF)),
    }
}

fn copiar_pasta(origem: &Path, destino: &Path) -> std::io::Result<()> {
    std::fs::create_dir(destino)?;
    for entrada in std::fs::read_dir(origem)? {
        let entrada = entrada?;
        let alvo = destino.join(entrada.file_name());
        if entrada.file_type()?.is_dir() {
            copiar_pasta(&entrada.path(), &alvo)?;
        } else {
            std::fs::copy(entrada.path(), &alvo)?;
        }
    }
    Ok(())
}

fn item_shell(p: &Path) -> windows::core::Result<IShellItem> {
    unsafe { SHCreateItemFromParsingName(&HSTRING::from(p.as_os_str()), None) }
}

/// Acrescenta uma linha por item em "_registro_usados.csv" (abre no Excel).
/// Serve para saber de onde cada arquivo veio, caso precise devolver.
fn registrar_csv(raiz: &Path, movidos: &[Movido], agora: &Agora) {
    let arquivo = raiz.join(config::NOME_REGISTRO_CSV);
    let novo = !arquivo.exists();

    let mut texto = String::new();
    if novo {
        texto.push('\u{FEFF}'); // BOM: faz o Excel reconhecer os acentos
        texto.push_str("Data;Tipo;Nome;Caminho original;Caminho novo\r\n");
    }
    let data = agora.legivel();
    for m in movidos {
        let nome = m.destino.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        texto.push_str(&format!(
            "{};{};{};{};{}\r\n",
            data,
            if m.eh_pasta { "Pasta" } else { "Arquivo" },
            util::csv(&nome),
            util::csv(&m.origem.to_string_lossy()),
            util::csv(&m.destino.to_string_lossy()),
        ));
    }

    // Se o CSV estiver aberto no Excel, ele fica travado; tenta algumas vezes.
    for _ in 0..5 {
        let ok = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&arquivo)
            .and_then(|mut f| f.write_all(texto.as_bytes()));
        if ok.is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(400));
    }
}
