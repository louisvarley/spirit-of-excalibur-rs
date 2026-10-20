//! SCENES.RES room records.
//!
//! Layout (verified against scene 1 and the prior notes):
//! `u8 object_count`, `u8 background`, `object_count * u32`, `u32 trailer`.
//! Object bits: 0..10 resource id, 11..19 `x + 190`, 20..27 `y`, 28..31 flags.

#![forbid(unsafe_code)]

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneObject {
    pub resource_id: u16,
    pub x: i16,
    pub y: u8,
    pub flags: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scene {
    pub background: u8,
    pub objects: Vec<SceneObject>,
    pub trailer: u32,
}

impl Scene {
    pub fn parse(data: &[u8]) -> Result<Self, String> {
        if data.len() < 2 {
            return Err("scene record too short".into());
        }
        let count = usize::from(data[0]);
        let trailer_at = 2 + count * 4;
        if data.len() < trailer_at {
            return Err(format!(
                "scene record too short: count={count}, len={}",
                data.len()
            ));
        }
        let objects = (0..count)
            .map(|i| {
                let o = 2 + i * 4;
                let v = u32::from_le_bytes([data[o], data[o + 1], data[o + 2], data[o + 3]]);
                SceneObject {
                    resource_id: (v & 0x7ff) as u16,
                    x: (((v >> 11) & 0x1ff) as i16) - 190,
                    y: ((v >> 20) & 0xff) as u8,
                    flags: ((v >> 28) & 0xf) as u8,
                }
            })
            .collect();
        let trailer = if data.len() >= trailer_at + 4 {
            u32::from_le_bytes([
                data[trailer_at],
                data[trailer_at + 1],
                data[trailer_at + 2],
                data[trailer_at + 3],
            ])
        } else {
            0
        };
        Ok(Self {
            background: data[1],
            objects,
            trailer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_1_matches_known_vector() {
        let data = std::fs::read("../../game/SCENES.RES");
        let Ok(file) = data else {
            eprintln!("SKIP: SCENES.RES absent");
            return;
        };
        let res = soe_res::ResFile::parse(file).unwrap();
        let rec = res.records().into_iter().find(|r| r.id == 1).unwrap();
        let scene = Scene::parse(&res.decode(rec).unwrap()).unwrap();
        assert_eq!(scene.background, 14);
        assert_eq!(scene.objects.len(), 16);
        assert_eq!(scene.trailer, 0x2a);
        let expected = [
            (1039, 0, 0, 0),
            (1403, 0, 100, 0),
            (1041, 183, 32, 0),
            (1041, -136, 31, 0),
            (1041, 0, 40, 0),
            (1407, 135, 32, 0),
        ];
        for (obj, (id, x, y, flags)) in scene.objects.iter().zip(expected) {
            assert_eq!(
                (obj.resource_id, obj.x, obj.y, obj.flags),
                (id, x, y, flags)
            );
        }
        assert_eq!(scene.objects[15].resource_id, 908);
    }
}
