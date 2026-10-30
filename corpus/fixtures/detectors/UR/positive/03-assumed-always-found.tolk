fun getValidatorDescr(validators: ValidatorDict, idx: int): ValidatorDescr {
    val res = validators.get(idx);
    // Assume, that the value is always found
    return res.loadValue();
}
