//! Fila de trabalho: junta vários arquivos selecionados num lote só.
//!
//! Quando você seleciona 50 arquivos e clica em "Marcar como usado", o Windows
//! abre o programa 50 vezes (uma para cada arquivo). Cada cópia só anota o seu
//! arquivo numa pasta de fila e tenta virar a "trabalhadora" (um mutex garante
//! que só uma consegue). A trabalhadora espera as anotações pararem de chegar e
//! move tudo de uma vez, com uma única janela de progresso do Windows.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_ABANDONED, WAIT_OBJECT_0};
use windows::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
use windows::core::w;

use crate::config;
use crate::mover;
use crate::util;

/// Tempo sem novidades na fila para considerar que o Windows terminou de abrir tudo.
const SILENCIO: Duration = Duration::from_millis(450);
/// Mesmo que continuem chegando itens, começa a mover depois disso.
const ESPERA_MAXIMA: Duration = Duration::from_secs(8);
/// Pedidos esquecidos na fila (ex.: o computador desligou no meio) são descartados.
/// É longo de propósito: pedidos feitos enquanto um HD externo lento ainda está
/// copiando ficam esperando a vez, e não podem ser jogados fora.
const VALIDADE: Duration = Duration::from_secs(12 * 60 * 60);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Modo {
    /// Mover para a pasta de usados configurada
    Padrao,
    /// Perguntar a pasta (uma vez para o lote inteiro)
    Escolher,
}

pub struct Pedido {
    pub modo: Modo,
    pub caminho: PathBuf,
}

fn pasta_fila() -> PathBuf {
    config::pasta_dados().join("fila")
}

/// Anota um item na fila. O arquivo é escrito como .tmp e renomeado no final,
/// assim a trabalhadora nunca lê um pedido pela metade.
pub fn enfileirar(modo: Modo, caminhos: &[PathBuf]) -> std::io::Result<()> {
    let pasta = pasta_fila();
    std::fs::create_dir_all(&pasta)?;
    let pid = std::process::id();
    for (i, caminho) in caminhos.iter().enumerate() {
        let caminho = std::path::absolute(caminho).unwrap_or_else(|_| caminho.clone());
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
        let nome = format!("{nanos:024}-{pid}-{i}");
        let mut conteudo = vec![match modo {
            Modo::Padrao => b'P',
            Modo::Escolher => b'E',
        }];
        conteudo.extend(util::caminho_para_bytes(&caminho));
        let tmp = pasta.join(format!("{nome}.tmp"));
        std::fs::write(&tmp, &conteudo)?;
        std::fs::rename(&tmp, pasta.join(format!("{nome}.pedido")))?;
    }
    Ok(())
}

fn listar_pedidos() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(pasta_fila())
        .map(|it| {
            it.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "pedido"))
                .collect()
        })
        .unwrap_or_default();
    v.sort(); // o nome começa com o horário → ordem de chegada
    v
}

fn ler_pedido(arquivo: &Path) -> Option<Pedido> {
    let dados = std::fs::read(arquivo);
    // Sai da fila mesmo se estiver estragado, para nunca ficar preso num pedido ruim.
    let _ = std::fs::remove_file(arquivo);
    let dados = dados.ok()?;
    let (&tipo, resto) = dados.split_first()?;
    let modo = match tipo {
        b'P' => Modo::Padrao,
        b'E' => Modo::Escolher,
        _ => return None,
    };
    // Descarta pedidos muito antigos
    let nanos: u128 = arquivo.file_stem()?.to_str()?.split('-').next()?.parse().ok()?;
    let agora = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    if agora.saturating_sub(nanos) > VALIDADE.as_nanos() {
        return None;
    }
    Some(Pedido { modo, caminho: util::bytes_para_caminho(resto) })
}

/// Espera o Windows terminar de abrir as cópias do programa e pega tudo o que está na fila.
fn coletar_lote() -> Vec<Pedido> {
    let inicio = Instant::now();
    let mut quantidade = listar_pedidos().len();
    loop {
        std::thread::sleep(SILENCIO);
        let agora = listar_pedidos().len();
        if agora == quantidade || inicio.elapsed() > ESPERA_MAXIMA {
            break;
        }
        quantidade = agora;
    }
    listar_pedidos().iter().filter_map(|p| ler_pedido(p)).collect()
}

struct Mutex(HANDLE);
impl Drop for Mutex {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Se nenhuma outra cópia estiver trabalhando, esta vira a trabalhadora e processa a fila.
/// Se já houver uma, sai na hora: a trabalhadora vai pegar o nosso pedido.
pub fn processar() {
    let Ok(h) = (unsafe { CreateMutexW(None, false, w!("Local\\Usados-Fila")) }) else {
        return;
    };
    let mutex = Mutex(h);
    let mut rodadas_vazias = 0;

    loop {
        let r = unsafe { WaitForSingleObject(mutex.0, 0) };
        if r != WAIT_OBJECT_0 && r != WAIT_ABANDONED {
            return; // outra cópia já está cuidando da fila
        }

        loop {
            let lote = coletar_lote();
            if lote.is_empty() {
                rodadas_vazias += 1;
                break;
            }
            rodadas_vazias = 0;
            mover::executar_lote(lote);
        }

        unsafe {
            let _ = ReleaseMutex(mutex.0);
        }
        // Algum pedido pode ter chegado bem na hora em que soltamos o mutex.
        // Se for o caso, tentamos pegar de novo (ou outra cópia já pegou).
        // (O limite de rodadas vazias é só uma proteção contra ficar rodando à toa.)
        if listar_pedidos().is_empty() || rodadas_vazias >= 3 {
            return;
        }
    }
}
