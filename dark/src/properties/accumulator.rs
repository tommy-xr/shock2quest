///
/// accumulator.rs
///
/// Functions for describing how properties are managed with inheritance
/// For most, they are simply overwritten - but some - like Scripts
/// need to potentially incorporate ancestor values

pub fn latest<T>(_ancestor_value: T, new_value: T) -> T {
    new_value
}
