use rope_editor::rope::Rope;

#[test]
fn concat_basico() {
    let a = Rope::new("Hola, ");
    let b = Rope::new("mundo!");
    let c = a.concat(&b);
    assert_eq!(c.to_string(), "Hola, mundo!");
    assert_eq!(c.len(), 12);
}

#[test]
fn split_preserva_contenido() {
    let r = Rope::new("Hola, mundo!");
    let (left, right) = r.split_at(6);
    assert_eq!(left.to_string(), "Hola, ");
    assert_eq!(right.to_string(), "mundo!");
}

#[test]
fn insert_no_muta_original() {
    let original = Rope::new("Hola mundo");
    let editado = original.insert(4, " lindo");
    assert_eq!(original.to_string(), "Hola mundo");
    assert_eq!(editado.to_string(), "Hola lindo mundo");
}

#[test]
fn delete_funciona() {
    let r = Rope::new("Hola lindo mundo");
    let sin_lindo = r.delete(4, 10);
    assert_eq!(sin_lindo.to_string(), "Hola mundo");
}

#[test]
fn rebalance_preserva_contenido() {
    let mut r = Rope::new("");
    for palabra in ["uno ", "dos ", "tres ", "cuatro"] {
        r = r.insert(r.len(), palabra);
    }
    let balanceado = r.rebalance();
    assert_eq!(r.to_string(), balanceado.to_string());
}

#[test]
fn char_at_funciona() {
    let r = Rope::new("Hola");
    assert_eq!(r.char_at(0), Some('H'));
    assert_eq!(r.char_at(3), Some('a'));
    assert_eq!(r.char_at(4), None);
}
