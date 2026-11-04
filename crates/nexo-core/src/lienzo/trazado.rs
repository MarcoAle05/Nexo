//! Coloca un diagrama sin coordenadas: trazado por niveles, como se dibuja un flujo a mano. El
//! agente da nodos y aristas, y aquí salen las posiciones. Es determinista, y nunca solapa cajas.

const BARRIDOS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caja {
    pub w: f64,
    pub h: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plano {
    /// La esquina de cada caja, en el orden en que llegaron, con el conjunto en (0, 0).
    pub pos: Vec<(f64, f64)>,
    pub w: f64,
    pub h: f64,
}

/// Un nivel para cada caja: el camino más largo desde una raíz. Las aristas que cierran un ciclo
/// no cuentan (un ciclo no tiene principio).
fn niveles(n: usize, aristas: &[(usize, usize)]) -> Vec<usize> {
    #[derive(Clone, Copy, PartialEq)]
    enum Marca {
        Nueva,
        Abierta,
        Cerrada,
    }
    let mut salen = vec![Vec::new(); n];
    for &(a, b) in aristas {
        if a != b && a < n && b < n {
            salen[a].push(b);
        }
    }
    // Quitar los ciclos: una arista hacia algo aún abierto vuelve atrás. Se recorre sin recursión,
    // porque el diagrama lo escribe un modelo y no conviene fiarse de su profundidad.
    let mut marca = vec![Marca::Nueva; n];
    let mut hacia_adelante: Vec<(usize, usize)> = Vec::new();
    let mut entran = vec![0usize; n];
    for &(a, b) in aristas {
        if a != b && a < n && b < n {
            entran[b] += 1;
        }
    }
    let raices: Vec<usize> = (0..n).filter(|i| entran[*i] == 0).chain(0..n).collect();
    for inicio in raices {
        if marca[inicio] != Marca::Nueva {
            continue;
        }
        let mut pila = vec![(inicio, 0usize)];
        marca[inicio] = Marca::Abierta;
        while let Some(&(nodo, i)) = pila.last() {
            if i < salen[nodo].len() {
                pila.last_mut().expect("pila").1 += 1;
                let hijo = salen[nodo][i];
                match marca[hijo] {
                    Marca::Abierta => {}
                    Marca::Cerrada => hacia_adelante.push((nodo, hijo)),
                    Marca::Nueva => {
                        hacia_adelante.push((nodo, hijo));
                        marca[hijo] = Marca::Abierta;
                        pila.push((hijo, 0));
                    }
                }
            } else {
                marca[nodo] = Marca::Cerrada;
                pila.pop();
            }
        }
    }
    let mut nivel = vec![0usize; n];
    // Como no quedan ciclos, n vueltas bastan.
    for _ in 0..n {
        let mut cambio = false;
        for &(a, b) in &hacia_adelante {
            if nivel[b] < nivel[a] + 1 {
                nivel[b] = nivel[a] + 1;
                cambio = true;
            }
        }
        if !cambio {
            break;
        }
    }
    nivel
}

/// Coloca las cajas. `derecha`: los niveles avanzan hacia la derecha; si no, hacia abajo.
/// `sep`: la separación entre niveles y dentro de un nivel.
pub fn por_niveles(
    cajas: &[Caja],
    aristas: &[(usize, usize)],
    derecha: bool,
    sep: (f64, f64),
) -> Plano {
    let n = cajas.len();
    if n == 0 {
        return Plano {
            pos: Vec::new(),
            w: 0.0,
            h: 0.0,
        };
    }
    let aristas: Vec<(usize, usize)> = aristas
        .iter()
        .copied()
        .filter(|(a, b)| a != b && *a < n && *b < n)
        .collect();
    // Sin aristas no hay niveles que valgan: una rejilla se lee mejor que una columna enorme.
    if aristas.is_empty() {
        return rejilla(cajas, sep.1);
    }
    let nivel = niveles(n, &aristas);
    let total = nivel.iter().max().copied().unwrap_or(0) + 1;
    let mut filas: Vec<Vec<usize>> = vec![Vec::new(); total];
    for i in 0..n {
        filas[nivel[i]].push(i);
    }
    // El orden dentro de cada nivel: cada caja cerca de sus vecinas, para que se crucen menos.
    let vecinos = |i: usize, atras: bool| -> Vec<usize> {
        aristas
            .iter()
            .filter_map(|&(a, b)| {
                if atras && b == i {
                    Some(a)
                } else if !atras && a == i {
                    Some(b)
                } else {
                    None
                }
            })
            .collect()
    };
    let mut puesto = vec![0.0f64; n];
    let fijar = |filas: &[Vec<usize>], puesto: &mut Vec<f64>| {
        for f in filas {
            for (p, &i) in f.iter().enumerate() {
                puesto[i] = p as f64;
            }
        }
    };
    fijar(&filas, &mut puesto);
    for barrido in 0..BARRIDOS {
        let atras = barrido % 2 == 0;
        let orden: Vec<usize> = if atras {
            (0..total).collect()
        } else {
            (0..total).rev().collect()
        };
        for t in orden {
            let mut clave: Vec<(f64, usize)> = filas[t]
                .iter()
                .enumerate()
                .map(|(p, &i)| {
                    let v = vecinos(i, atras);
                    let k = if v.is_empty() {
                        p as f64
                    } else {
                        v.iter().map(|x| puesto[*x]).sum::<f64>() / v.len() as f64
                    };
                    (k, i)
                })
                .collect();
            clave.sort_by(|a, b| a.0.total_cmp(&b.0));
            filas[t] = clave.into_iter().map(|(_, i)| i).collect();
            for (p, &i) in filas[t].iter().enumerate() {
                puesto[i] = p as f64;
            }
        }
    }
    // Eje principal: un nivel tras otro, tan ancho como su caja mayor.
    let principal = |c: &Caja| if derecha { c.w } else { c.h };
    let cruzado = |c: &Caja| if derecha { c.h } else { c.w };
    let mut en_principal = vec![0.0f64; n];
    let mut avance = 0.0;
    for f in &filas {
        let grueso = f.iter().map(|&i| principal(&cajas[i])).fold(0.0, f64::max);
        for &i in f {
            en_principal[i] = avance + (grueso - principal(&cajas[i])) / 2.0;
        }
        avance += grueso + sep.0;
    }
    // Eje cruzado: cada caja frente a sus vecinas, sin dejar de respetar el orden y la separación.
    let mut centro = vec![0.0f64; n];
    for f in &filas {
        let mut c = 0.0;
        for &i in f {
            centro[i] = c + cruzado(&cajas[i]) / 2.0;
            c += cruzado(&cajas[i]) + sep.1;
        }
    }
    for barrido in 0..BARRIDOS {
        let atras = barrido % 2 == 0;
        let orden: Vec<usize> = if atras {
            (0..total).collect()
        } else {
            (0..total).rev().collect()
        };
        for t in orden {
            let f = &filas[t];
            let mut quiere: Vec<f64> = f
                .iter()
                .map(|&i| {
                    let v = vecinos(i, atras);
                    if v.is_empty() {
                        centro[i]
                    } else {
                        v.iter().map(|x| centro[*x]).sum::<f64>() / v.len() as f64
                    }
                })
                .collect();
            for k in 1..f.len() {
                let hueco = cruzado(&cajas[f[k - 1]]) / 2.0 + sep.1 + cruzado(&cajas[f[k]]) / 2.0;
                if quiere[k] < quiere[k - 1] + hueco {
                    quiere[k] = quiere[k - 1] + hueco;
                }
            }
            for (k, &i) in f.iter().enumerate() {
                centro[i] = quiere[k];
            }
        }
    }
    let minimo = (0..n)
        .map(|i| centro[i] - cruzado(&cajas[i]) / 2.0)
        .fold(f64::INFINITY, f64::min);
    let reticula = |v: f64| (v / 22.0).round() * 22.0;
    let mut pos = vec![(0.0, 0.0); n];
    for i in 0..n {
        let c = reticula(centro[i] - cruzado(&cajas[i]) / 2.0 - minimo);
        let p = reticula(en_principal[i]);
        pos[i] = if derecha { (p, c) } else { (c, p) };
    }
    // Ajustar a la retícula puede comerse hasta 22 px de una separación; si esta es menor, se
    // empuja para que ninguna caja llegue a pisar a otra.
    for f in &filas {
        for k in 1..f.len() {
            let (a, b) = (f[k - 1], f[k]);
            if derecha {
                let fin = pos[a].1 + cajas[a].h;
                if pos[b].1 < fin + 22.0 {
                    pos[b].1 = reticula(fin + 22.0 + 11.0);
                }
            } else {
                let fin = pos[a].0 + cajas[a].w;
                if pos[b].0 < fin + 22.0 {
                    pos[b].0 = reticula(fin + 22.0 + 11.0);
                }
            }
        }
    }
    medir(pos, cajas)
}

fn rejilla(cajas: &[Caja], sep: f64) -> Plano {
    let columnas = (cajas.len() as f64).sqrt().ceil() as usize;
    let filas = cajas.len().div_ceil(columnas);
    let mut anchos = vec![0.0f64; columnas];
    let mut altos = vec![0.0f64; filas];
    for (i, c) in cajas.iter().enumerate() {
        anchos[i % columnas] = anchos[i % columnas].max(c.w);
        altos[i / columnas] = altos[i / columnas].max(c.h);
    }
    let mut pos = Vec::with_capacity(cajas.len());
    for i in 0..cajas.len() {
        let x: f64 = anchos[..i % columnas].iter().map(|a| a + sep).sum();
        let y: f64 = altos[..i / columnas].iter().map(|a| a + sep).sum();
        pos.push((x, y));
    }
    medir(pos, cajas)
}

fn medir(pos: Vec<(f64, f64)>, cajas: &[Caja]) -> Plano {
    let w = pos
        .iter()
        .zip(cajas)
        .map(|(p, c)| p.0 + c.w)
        .fold(0.0, f64::max);
    let h = pos
        .iter()
        .zip(cajas)
        .map(|(p, c)| p.1 + c.h)
        .fold(0.0, f64::max);
    Plano { pos, w, h }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caja(w: f64, h: f64) -> Caja {
        Caja { w, h }
    }

    fn sin_solapes(p: &Plano, cajas: &[Caja]) {
        for i in 0..cajas.len() {
            for j in i + 1..cajas.len() {
                let (a, b) = (p.pos[i], p.pos[j]);
                let libres = a.0 + cajas[i].w <= b.0
                    || b.0 + cajas[j].w <= a.0
                    || a.1 + cajas[i].h <= b.1
                    || b.1 + cajas[j].h <= a.1;
                assert!(libres, "{i} y {j} se pisan: {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn una_cadena_avanza_nivel_a_nivel() {
        let cajas = [caja(132.0, 66.0); 3];
        let p = por_niveles(&cajas, &[(0, 1), (1, 2)], true, (66.0, 44.0));
        assert!(p.pos[0].0 < p.pos[1].0 && p.pos[1].0 < p.pos[2].0);
        assert_eq!(p.pos[0].1, p.pos[1].1, "una cadena va recta");
        let p = por_niveles(&cajas, &[(0, 1), (1, 2)], false, (66.0, 44.0));
        assert!(p.pos[0].1 < p.pos[1].1 && p.pos[1].1 < p.pos[2].1);
        sin_solapes(&p, &cajas);
    }

    #[test]
    fn un_ciclo_o_una_arista_a_si_misma_no_rompen_nada() {
        let cajas = [caja(132.0, 66.0); 4];
        let p = por_niveles(
            &cajas,
            &[(0, 1), (1, 2), (2, 0), (2, 2), (3, 3), (9, 1)],
            true,
            (66.0, 44.0),
        );
        assert_eq!(p.pos.len(), 4);
        sin_solapes(&p, &cajas);
    }

    #[test]
    fn con_ramas_y_tamanos_distintos_ninguna_caja_pisa_a_otra() {
        let cajas = [
            caja(132.0, 66.0),
            caja(264.0, 154.0),
            caja(132.0, 44.0),
            caja(176.0, 88.0),
            caja(132.0, 66.0),
            caja(220.0, 110.0),
        ];
        let aristas = [(0, 1), (0, 2), (0, 3), (1, 4), (2, 4), (3, 5), (4, 5)];
        for derecha in [true, false] {
            let p = por_niveles(&cajas, &aristas, derecha, (66.0, 44.0));
            sin_solapes(&p, &cajas);
            for (a, b) in aristas {
                let eje = |q: (f64, f64)| if derecha { q.0 } else { q.1 };
                assert!(eje(p.pos[a]) < eje(p.pos[b]), "{a} va antes que {b}");
            }
        }
    }

    #[test]
    fn sin_aristas_sale_una_rejilla_y_es_determinista() {
        let cajas = [caja(132.0, 66.0); 9];
        let p = por_niveles(&cajas, &[], true, (66.0, 44.0));
        sin_solapes(&p, &cajas);
        assert!(p.w < 9.0 * 132.0 && p.h < 9.0 * 66.0);
        assert_eq!(p, por_niveles(&cajas, &[], true, (66.0, 44.0)));
    }

    #[test]
    fn nada_que_colocar() {
        assert_eq!(
            por_niveles(&[], &[], true, (66.0, 44.0)),
            Plano {
                pos: vec![],
                w: 0.0,
                h: 0.0
            }
        );
    }
}
