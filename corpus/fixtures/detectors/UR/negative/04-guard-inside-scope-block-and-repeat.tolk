// Форма, найденная циклом фазы 4 на реальном production-коде:
// `smartcontractkit/chainlink-ton`, `contracts/contracts/mcms/mcms.tolk`
// (функции `setRoot`/`setConfig`). Написана нами по мотивам, не скопирована —
// вендоринг чужого кода в corpus/ запрещён фазой 4 §8.
//
// Проверка `isFound` стоит на месте, но она и вместе с ней `loadValue()`
// завёрнуты в голый блок области видимости `{ ... }`, а внешняя обёртка —
// `repeat`. До фикса `UR` не проходил обходом ни через тот, ни через другой и
// сообщал 11 ложных срабатываний на одном этом файле.

fun sumFirstQuorums(groupQuorums: map<int, int>, groupParents: map<int, int>): int {
    var total = 0;
    var index = 0;

    repeat(4) {
        // Голый блок ограничивает область видимости `value` — распространённый
        // приём в больших контрактах, где одно и то же имя переиспользуется.
        {
            var value = groupQuorums.get(index);
            if (value.isFound) {
                total += value.loadValue();
            }
        }

        {
            var value = groupParents.get(index);
            if (!value.isFound) {
                throw 101;
            }
            total += value.loadValue();
        }

        index += 1;
    }

    return total;
}
