fun getValidatorDescr(validators: ValidatorDict, idx: int): ValidatorDescr {
    val lookup = validators.get(idx);
    assert (lookup.isFound) throw ERROR_WRONG_VALIDATOR_DESCRIPTION;
    return lookup.loadValue();
}
