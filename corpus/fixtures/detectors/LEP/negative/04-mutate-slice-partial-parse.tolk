fun DirectIndexedAddressMap.unpackFromSlice(mutate s: slice): DirectIndexedAddressMap {
    return s.loadRef();
}
