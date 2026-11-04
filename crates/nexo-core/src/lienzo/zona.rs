//! Dónde cabe algo: el hueco libre que el motor le da al agente para dibujar, sin tocar nada de lo
//! que hay. Es determinista: con el mismo lienzo, la misma respuesta.

use serde::Serialize;

use super::{Lienzo, MUNDO};

const MARGEN: f64 = 44.0;
const VIGENCIA_MS: u64 = 2 * 60 * 1000;
const RETICULA: f64 = 22.0;

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn toca(&self, o: &Rect, margen: f64) -> bool {
        self.x < o.x + o.w + margen
            && self.x + self.w + margen > o.x
            && self.y < o.y + o.h + margen
            && self.y + self.h + margen > o.y
    }

    pub fn centro(&self) -> (f64, f64) {
        (self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// Está entero dentro de `o`.
    pub fn dentro_de(&self, o: &Rect) -> bool {
        self.x >= o.x
            && self.y >= o.y
            && self.x + self.w <= o.x + o.w
            && self.y + self.h <= o.y + o.h
    }
}

// La retícula de puntos del lienzo está en el punto (0, 0) de la superficie, que en coordenadas
// del lienzo es la esquina del mundo.
fn reticula(v: f64, medio: f64) -> f64 {
    ((v + medio) / RETICULA).round() * RETICULA - medio
}
pub fn reticula_x(v: f64) -> f64 {
    reticula(v, MUNDO.0 / 2.0)
}
pub fn reticula_y(v: f64) -> f64 {
    reticula(v, MUNDO.1 / 2.0)
}

pub(crate) fn abarca(rects: &[Rect]) -> Option<Rect> {
    let x = rects.iter().map(|r| r.x).reduce(f64::min)?;
    let y = rects.iter().map(|r| r.y).reduce(f64::min)?;
    let d = rects.iter().map(|r| r.x + r.w).reduce(f64::max)?;
    let b = rects.iter().map(|r| r.y + r.h).reduce(f64::max)?;
    Some(Rect {
        x,
        y,
        w: d - x,
        h: b - y,
    })
}

impl Lienzo {
    /// Lo que ocupa sitio: los elementos, las cartas y lo que ya se le prometió a alguien. Una
    /// flecha entre dos elementos no ocupa: va por donde ellos.
    fn obstaculos(&self) -> Vec<Rect> {
        let mut out: Vec<Rect> = self
            .elementos
            .iter()
            .filter(|e| {
                e.tipo != "flecha"
                    || e.datos["desde"].get("id").is_none()
                    || e.datos["hasta"].get("id").is_none()
            })
            .map(|e| self.rect_de(e))
            .collect();
        out.extend(self.cartas.iter().map(|(_, r)| *r));
        out.extend(self.reservas.iter().map(|(r, _)| *r));
        out
    }

    /// El hueco para `w`×`h`: junto a `cerca` si se dice y, si no, junto a lo que hay. Con el
    /// lienzo vacío, donde el usuario está mirando.
    pub fn zona_libre(&self, w: f64, h: f64, cerca: Option<&str>) -> Rect {
        let (w, h) = (w.max(22.0), h.max(22.0));
        let obstaculos = self.obstaculos();
        let en_mundo = |x: f64, y: f64| {
            x >= -MUNDO.0 / 2.0
                && y >= -MUNDO.1 / 2.0
                && x + w <= MUNDO.0 / 2.0
                && y + h <= MUNDO.1 / 2.0
        };
        let encaja = |x: f64, y: f64| {
            let r = Rect { x, y, w, h };
            en_mundo(x, y) && !obstaculos.iter().any(|o| r.toca(o, MARGEN))
        };
        let Some(todo) = abarca(&obstaculos) else {
            // La vista es lo que hay en la esquina de la ventana, tapada por la barra y la carta
            // izquierda: se deja ese margen para que caiga en el hueco libre.
            let (vx, vy) = self.vista.unwrap_or((-22.0, -22.0));
            let (x, y) = if self.vista.is_some() {
                (vx + 396.0, vy + 110.0)
            } else {
                (22.0, 22.0)
            };
            return Rect {
                x: reticula_x(x),
                y: reticula_y(y),
                w,
                h,
            };
        };
        let objetivo = cerca
            .and_then(|id| self.elemento(id))
            .map(|e| self.rect_de(e))
            .unwrap_or(todo);
        let (ox, oy) = objetivo.centro();
        // Candidatos: a la derecha y debajo de cada cosa, y a la derecha y debajo de todo.
        let mut candidatos: Vec<(f64, f64)> = vec![
            (todo.x + todo.w + MARGEN, todo.y),
            (todo.x, todo.y + todo.h + MARGEN),
        ];
        for o in &obstaculos {
            candidatos.push((o.x + o.w + MARGEN, o.y));
            candidatos.push((o.x, o.y + o.h + MARGEN));
        }
        let mut mejor: Option<(f64, f64, f64)> = None;
        for (x, y) in candidatos {
            // Se ajusta hacia afuera para no comerse el margen al caer en la retícula.
            let (x, y) = (
                ((x + MUNDO.0 / 2.0) / RETICULA).ceil() * RETICULA - MUNDO.0 / 2.0,
                ((y + MUNDO.1 / 2.0) / RETICULA).ceil() * RETICULA - MUNDO.1 / 2.0,
            );
            if !encaja(x, y) {
                continue;
            }
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            // Se lee de izquierda a derecha: a igual distancia, mejor a la derecha que debajo.
            let d = (cx - ox).abs() + (cy - oy).abs() * 1.4;
            if mejor.is_none_or(|(m, mx, my)| d < m || (d == m && (x, y) < (mx, my))) {
                mejor = Some((d, x, y));
            }
        }
        let (x, y) = match mejor {
            Some((_, x, y)) => (x, y),
            None => (reticula_x(todo.x), reticula_y(todo.y + todo.h + MARGEN)),
        };
        Rect { x, y, w, h }
    }

    /// Lo mismo, y se lo guarda: dos peticiones seguidas no reciben el mismo hueco, aunque aún no
    /// se haya dibujado nada en la primera. La promesa caduca sola.
    pub fn reservar_zona(&mut self, w: f64, h: f64, cerca: Option<&str>, ahora: u64) -> Rect {
        self.reservas
            .retain(|(_, t)| ahora.saturating_sub(*t) < VIGENCIA_MS);
        let zona = self.zona_libre(w, h, cerca);
        self.reservas.push((zona, ahora));
        zona
    }
}
