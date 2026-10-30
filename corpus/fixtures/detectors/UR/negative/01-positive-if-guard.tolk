fun getConfigValueOrNull(config: ConfigDict, idx: int32): cell? {
    val lookup = config.get(idx);
    if (lookup.isFound) {
        return lookup.loadValue();
    }
    return null;
}
