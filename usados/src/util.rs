//! Pequenas funções auxiliares: caminhos, datas e texto.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::{Path, PathBuf};

use windows::Win32::System::SystemInformation::GetLocalTime;

/// %LOCALAPPDATA% (ex.: C:\Users\Fulano\AppData\Local)
pub fn local_appdata() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir())
}

/// Caminho normalizado para comparação: absoluto, sem "\\?\", minúsculo e terminado em "\".
pub fn chave_caminho(p: &Path) -> String {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut s = abs.to_string_lossy().replace('/', "\\");
    if let Some(r) = s.strip_prefix(r"\\?\UNC\") {
        s = format!(r"\\{r}");
    } else if let Some(r) = s.strip_prefix(r"\\?\") {
        s = r.to_string();
    }
    let mut s = s.to_lowercase();
    if !s.ends_with('\\') {
        s.push('\\');
    }
    s
}

/// `filho` está dentro de `pai` (ou é o próprio `pai`)?
pub fn esta_dentro(filho: &Path, pai: &Path) -> bool {
    chave_caminho(filho).starts_with(&chave_caminho(pai))
}

pub fn mesmo_caminho(a: &Path, b: &Path) -> bool {
    chave_caminho(a) == chave_caminho(b)
}

/// Data/hora local do Windows.
pub struct Agora {
    pub ano: u16,
    pub mes: u16,
    pub dia: u16,
    pub hora: u16,
    pub minuto: u16,
    pub segundo: u16,
}

pub fn agora() -> Agora {
    let st = unsafe { GetLocalTime() };
    Agora {
        ano: st.wYear,
        mes: st.wMonth,
        dia: st.wDay,
        hora: st.wHour,
        minuto: st.wMinute,
        segundo: st.wSecond,
    }
}

impl Agora {
    pub fn ano_mes(&self) -> String {
        format!("{:04}-{:02}", self.ano, self.mes)
    }
    pub fn ano_mes_dia(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.ano, self.mes, self.dia)
    }
    /// Formato brasileiro para o registro: 24/09/2026 14:30:05
    pub fn legivel(&self) -> String {
        format!(
            "{:02}/{:02}/{:04} {:02}:{:02}:{:02}",
            self.dia, self.mes, self.ano, self.hora, self.minuto, self.segundo
        )
    }
}

/// Divide "video.final.mp4" em ("video.final", ".mp4"). Pastas não têm extensão.
fn separar_extensao(nome: &str, eh_pasta: bool) -> (&str, &str) {
    if eh_pasta {
        return (nome, "");
    }
    match nome.rfind('.') {
        Some(i) if i > 0 => (&nome[..i], &nome[i..]),
        _ => (nome, ""),
    }
}

/// Escolhe um nome que ainda não existe em `pasta`: "clip.mp4", "clip (2).mp4", "clip (3).mp4"...
/// `reservados` evita que dois itens do mesmo lote recebam o mesmo nome.
pub fn nome_livre(
    pasta: &Path,
    nome: &OsStr,
    eh_pasta: bool,
    reservados: &mut std::collections::HashSet<String>,
) -> OsString {
    let livre = |candidato: &OsStr, reservados: &std::collections::HashSet<String>| {
        let completo = pasta.join(candidato);
        !completo.exists() && !reservados.contains(&chave_caminho(&completo))
    };

    if livre(nome, reservados) {
        reservados.insert(chave_caminho(&pasta.join(nome)));
        return nome.to_os_string();
    }

    let texto = nome.to_string_lossy().into_owned();
    let (base, ext) = separar_extensao(&texto, eh_pasta);
    for n in 2u32.. {
        let candidato = OsString::from(format!("{base} ({n}){ext}"));
        if livre(&candidato, reservados) {
            reservados.insert(chave_caminho(&pasta.join(&candidato)));
            return candidato;
        }
    }
    unreachable!()
}

/// Codifica um caminho em UTF-16 sem perdas (inclusive nomes com caracteres estranhos).
pub fn caminho_para_bytes(p: &Path) -> Vec<u8> {
    p.as_os_str()
        .encode_wide()
        .flat_map(|c| c.to_le_bytes())
        .collect()
}

pub fn bytes_para_caminho(b: &[u8]) -> PathBuf {
    let wide: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    PathBuf::from(OsString::from_wide(&wide))
}

/// Encurta caminhos longos para caber no menu: "D:\Trabalho\...\Usados".
pub fn encurtar(texto: &str, max: usize) -> String {
    let chars: Vec<char> = texto.chars().collect();
    if chars.len() <= max {
        return texto.to_string();
    }
    let lado = (max.saturating_sub(1)) / 2;
    let ini: String = chars[..lado].iter().collect();
    let fim: String = chars[chars.len() - lado..].iter().collect();
    format!("{ini}…{fim}")
}

/// Campo de CSV (separador ";", que é o padrão do Excel em português).
pub fn csv(campo: &str) -> String {
    if campo.contains([';', '"', '\n', '\r']) {
        format!("\"{}\"", campo.replace('"', "\"\""))
    } else {
        campo.to_string()
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn extensao() {
        assert_eq!(separar_extensao("clip.mp4", false), ("clip", ".mp4"));
        assert_eq!(separar_extensao("a.b.mov", false), ("a.b", ".mov"));
        assert_eq!(separar_extensao(".hidden", false), (".hidden", ""));
        assert_eq!(separar_extensao("Projeto.v2", true), ("Projeto.v2", ""));
    }

    #[test]
    fn dentro() {
        assert!(esta_dentro(Path::new(r"D:\Usados\x.mp4"), Path::new(r"d:\usados")));
        assert!(esta_dentro(Path::new(r"D:\Usados"), Path::new(r"D:\Usados\")));
        assert!(!esta_dentro(Path::new(r"D:\Usados2\x.mp4"), Path::new(r"D:\Usados")));
    }

    #[test]
    fn nomes_repetidos() {
        let pasta = std::env::temp_dir().join(format!("usados-teste-{}", std::process::id()));
        std::fs::create_dir_all(&pasta).unwrap();
        std::fs::write(pasta.join("clip.mp4"), b"x").unwrap();
        std::fs::create_dir_all(pasta.join("Projeto")).unwrap();
        let mut r = std::collections::HashSet::new();
        assert_eq!(nome_livre(&pasta, OsStr::new("clip.mp4"), false, &mut r), "clip (2).mp4");
        assert_eq!(nome_livre(&pasta, OsStr::new("clip.mp4"), false, &mut r), "clip (3).mp4");
        assert_eq!(nome_livre(&pasta, OsStr::new("novo.mov"), false, &mut r), "novo.mov");
        assert_eq!(nome_livre(&pasta, OsStr::new("novo.mov"), false, &mut r), "novo (2).mov");
        assert_eq!(nome_livre(&pasta, OsStr::new("Projeto"), true, &mut r), "Projeto (2)");
        std::fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn encurta() {
        assert_eq!(encurtar("abc", 10), "abc");
        assert_eq!(encurtar("0123456789ABCDEF", 9).chars().count(), 9);
    }
}
