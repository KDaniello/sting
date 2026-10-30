fun getConfigValue(config: ConfigDict, idx: int32): cell {
    val lookup = config.get(idx);
    return lookup.loadValue();
}
