fun ConfigDict.getById(self, index: int32): cell? {
    val lookup = self.get(index);
    if (!lookup.isFound) {
        return null;
    }
    return lookup.loadValue();
}
