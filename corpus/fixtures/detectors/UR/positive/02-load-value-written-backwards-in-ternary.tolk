fun getConfigValueOrDefault(config: ConfigDict, idx: int32): cell {
    val lookup = config.get(idx);
    return lookup.isFound ? null : lookup.loadValue();
}
