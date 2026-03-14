use typed_index_collections::TiSlice;

pub fn ti_iter<K, V>(ti: &TiSlice<K, V>) -> impl Iterator<Item = (K, &V)>
where
    K: From<usize>,
{
    ti.iter().enumerate().map(|(i, v)| (K::from(i), v))
}
