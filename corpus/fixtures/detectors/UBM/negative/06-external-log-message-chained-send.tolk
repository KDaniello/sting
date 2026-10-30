// Форма, найденная циклом фазы 4 на реальном production-коде:
// `xssnick/payment-channel-contract`, `contracts/universal/`. Написана нами по
// мотивам, не скопирована — вендоринг чужого кода запрещён фазой 4 §8.
//
// Событие-лог отправляется ЦЕПОЧКОЙ, без промежуточной переменной.
// `createExternalLogMessage` строит `ext_out_msg_info` — сообщение, не
// способное отскочить в принципе, поэтому находки здесь быть не должно.
// Прежний фикс распознавателя закрывал только форму «через переменную»: обход
// преордерный, и внешний `.send(...)` посещался раньше вложенного
// `createExternalLogMessage`, отчего живой платёжный канал получал ложное
// Critical.

fun onInternalMessage(in: InMessage) {
    var storage = lazy Storage.load();
    // Проверка отправителя стоит здесь не для UBM, а чтобы фикстура не
    // порождала ложное срабатывание AC: корпус, собранный под один детектор и
    // шумящий в другом, — известная болезнь этого проекта (18 из 32 FP AC —
    // ровно такие артефакты).
    assert (in.senderAddress == storage.ownerAddress) throw 401;
    storage.counter += 1;
    storage.save();

    createExternalLogMessage({
        dest: createAddressNone(),
        body: CounterChanged { value: storage.counter },
    }).send(SEND_MODE_REGULAR);
}
