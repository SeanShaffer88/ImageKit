#[test]
fn bounded_deflate_checks_happen_before_growth() {
    let payload = vec![b'a'; 4096];
    let stream = imagekit::deflate::deflate_zlib(&payload);
    assert_eq!(imagekit::inflate::unzlib(&stream, 4096).unwrap(), payload);
    assert!(imagekit::inflate::unzlib(&stream, 4095).is_err());
    assert!(imagekit::inflate::inflate(&[1, 1, 0, 254, 255, 42], 0).is_err());
}

#[test]
fn hostile_dimensions_are_refused_before_pixel_allocation() {
    assert!(imagekit::image::Image::solid(u32::MAX, u32::MAX, [0; 4]).is_err());
    assert!(imagekit::decode(b"P6\n4294967295 4294967295\n255\n").is_err());
    let mut bmp = vec![0u8; 54];
    bmp[0..2].copy_from_slice(b"BM");
    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&i32::MAX.to_le_bytes());
    bmp[22..26].copy_from_slice(&i32::MAX.to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    assert!(imagekit::decode(&bmp).is_err());
}
#[test]
fn bounded_mutations_and_truncations_do_not_panic() {
    let image = imagekit::image::Image::new(2, 2, vec![17; 16], "test").unwrap();
    let original = imagekit::png::encode_png(&image);
    for len in 0..original.len() {
        assert!(imagekit::decode(&original[..len]).is_err());
    }
    for i in 0..original.len() {
        let mut bytes = original.clone();
        bytes[i] ^= 0x55;
        let _ = imagekit::decode(&bytes);
    }
    let mut seed = 0x98765432u32;
    for len in 0..256 {
        let data: Vec<u8> = (0..len)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 17;
                seed ^= seed << 5;
                seed as u8
            })
            .collect();
        let _ = imagekit::decode(&data);
        let _ = imagekit::inflate::inflate(&data, 1024);
    }
}
