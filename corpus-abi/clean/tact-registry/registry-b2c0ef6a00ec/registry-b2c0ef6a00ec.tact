import "@stdlib/deploy";

// ═══════════════════════════════════════════════════════════════════════════════
// Messages
// ═══════════════════════════════════════════════════════════════════════════════

message RegisterTenant {
    tenantId: Int as uint128;
    payoutAddress: Address;
}

message AddItem {
    tenantId: Int as uint128;
    itemId: Int as uint32;
    price: Int as uint64;
}

message PayWithTon {
    tenantId: Int as uint128;
    itemId: Int as uint32;
    referrerAddress: Address?;
    referrerFeeBps: Int as uint16;
    supervisorAddress: Address?;
    supervisorFeeBps: Int as uint16;
}

message UpdateTenant {
    tenantId: Int as uint128;
    newPayoutAddress: Address;
    active: Bool;
}

message AdminUpdate {
    platformAddress: Address?;
    platformFeeBps: Int?;
    usdtMasterAddress: Address?;
    usdtWalletAddress: Address?;
    active: Bool?;
}

message WithdrawJetton {
    amount: Int as coins;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Events (emit)
// ═══════════════════════════════════════════════════════════════════════════════

message TenantRegistered {
    tenantId: Int as uint128;
    ownerAddress: Address;
    payoutAddress: Address;
}

message ItemAdded {
    tenantId: Int as uint128;
    itemId: Int as uint32;
    price: Int as uint64;
}

message PaymentProcessed {
    tenantId: Int as uint128;
    itemId: Int as uint32;
    payer: Address;
    amount: Int as coins;
    currency: Int as uint8;     // 0 = TON, 1 = USDT
}

message PaymentComment {
    tenantId: Int as uint128;
    itemId: Int as uint32;
    role: Int as uint8;         // 0=platform, 1=referrer, 2=supervisor, 3=tenant
}

message AdminUpdated {}

// ═══════════════════════════════════════════════════════════════════════════════
// Jetton TEP-74
// ═══════════════════════════════════════════════════════════════════════════════

message(0x7362d09c) JettonTransferNotification {
    queryId: Int as uint64;
    amount: Int as coins;
    sender: Address;
    forwardPayload: Slice as remaining;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Structs
// ═══════════════════════════════════════════════════════════════════════════════

struct TenantConfig {
    ownerAddress: Address;
    payoutAddress: Address;
    active: Bool;
}

struct ItemConfig {
    price: Int as uint64;
    active: Bool;
}

// ═══════════════════════════════════════════════════════════════════════════════
// Contract
// ═══════════════════════════════════════════════════════════════════════════════

contract UniGenPayment with Deployable {

    owner: Address;
    platformAddress: Address;
    platformFeeBps: Int as uint16;
    usdtMasterAddress: Address;
    myUsdtWalletAddress: Address;
    tenants: map<Int, TenantConfig>;
    items: map<Int, ItemConfig>;
    active: Bool;

    init(
        owner: Address,
        platformAddress: Address,
        platformFeeBps: Int,
        usdtMasterAddress: Address
    ) {
        self.owner = owner;
        self.platformAddress = platformAddress;
        self.platformFeeBps = platformFeeBps;
        self.usdtMasterAddress = usdtMasterAddress;
        self.myUsdtWalletAddress = newAddress(0, 0); // placeholder — set via AdminUpdate after deploy
        self.tenants = emptyMap();
        self.items = emptyMap();
        self.active = true;
    }

    // ─── RegisterTenant ──────────────────────────────────────────────────────

    receive(msg: RegisterTenant) {
        require(self.active, "Contract is paused");
        require(msg.tenantId != 0, "Invalid tenantId");
        require(self.tenants.get(msg.tenantId) == null, "Tenant already registered");
        require(context().value >= ton("0.05"), "Insufficient TON for storage fee");

        self.tenants.set(msg.tenantId, TenantConfig{
            ownerAddress: sender(),
            payoutAddress: msg.payoutAddress,
            active: true
        });

        emit(TenantRegistered{
            tenantId: msg.tenantId,
            ownerAddress: sender(),
            payoutAddress: msg.payoutAddress
        }.toCell());

        self.reply(null);
    }

    // ─── AddItem ─────────────────────────────────────────────────────────────

    receive(msg: AddItem) {
        require(self.active, "Contract is paused");

        let tenant: TenantConfig? = self.tenants.get(msg.tenantId);
        require(tenant != null, "Tenant not found");
        require(tenant!!.ownerAddress == sender(), "Not tenant owner");
        require(tenant!!.active, "Tenant not active");
        require(context().value >= ton("0.02"), "Insufficient TON for storage fee");

        let itemKey: Int = (msg.tenantId << 32) | msg.itemId;
        self.items.set(itemKey, ItemConfig{
            price: msg.price,
            active: true
        });

        emit(ItemAdded{
            tenantId: msg.tenantId,
            itemId: msg.itemId,
            price: msg.price
        }.toCell());
    }

    // ─── PayWithTon ──────────────────────────────────────────────────────────

    receive(msg: PayWithTon) {
        require(self.active, "Contract is paused");

        let tenant: TenantConfig? = self.tenants.get(msg.tenantId);
        require(tenant != null, "Tenant not found");
        require(tenant!!.active, "Tenant not active");

        let sumBps: Int = self.platformFeeBps + msg.referrerFeeBps + msg.supervisorFeeBps;
        require(sumBps <= 10000, "BPS overflow");

        let gasReserve: Int = ton("0.04");
        let totalAmount: Int = context().value - gasReserve;
        require(totalAmount > 0, "Insufficient amount");

        // Step 1: Platform
        let platformAmount: Int = totalAmount * self.platformFeeBps / 10000;
        send(SendParameters{
            to: self.platformAddress,
            bounce: false,
            value: platformAmount,
            mode: SendIgnoreErrors,
            body: PaymentComment{ tenantId: msg.tenantId, itemId: msg.itemId, role: 0 }.toCell()
        });
        let remainder: Int = totalAmount - platformAmount;

        // Step 2: Referrer
        if (msg.referrerAddress != null && msg.referrerFeeBps > 0 && remainder > 0) {
            let referrerAmount: Int = totalAmount * msg.referrerFeeBps / 10000;
            send(SendParameters{
                to: msg.referrerAddress!!,
                bounce: false,
                value: referrerAmount,
                mode: SendIgnoreErrors,
                body: PaymentComment{ tenantId: msg.tenantId, itemId: msg.itemId, role: 1 }.toCell()
            });
            remainder = remainder - referrerAmount;
        }

        // Step 3: Supervisor
        if (msg.supervisorAddress != null && msg.supervisorFeeBps > 0 && remainder > 0) {
            let supervisorAmount: Int = totalAmount * msg.supervisorFeeBps / 10000;
            send(SendParameters{
                to: msg.supervisorAddress!!,
                bounce: false,
                value: supervisorAmount,
                mode: SendIgnoreErrors,
                body: PaymentComment{ tenantId: msg.tenantId, itemId: msg.itemId, role: 2 }.toCell()
            });
            remainder = remainder - supervisorAmount;
        }

        // Step 4: Tenant gets remainder
        if (remainder > ton("0.001")) {
            send(SendParameters{
                to: tenant!!.payoutAddress,
                bounce: false,
                value: remainder,
                mode: SendIgnoreErrors,
                body: PaymentComment{ tenantId: msg.tenantId, itemId: msg.itemId, role: 3 }.toCell()
            });
        }

        emit(PaymentProcessed{
            tenantId: msg.tenantId,
            itemId: msg.itemId,
            payer: sender(),
            amount: context().value,
            currency: 0
        }.toCell());
    }

    // ─── JettonTransferNotification (USDT) ───────────────────────────────────

    receive(msg: JettonTransferNotification) {
        require(self.active, "Contract is paused");
        require(sender() == self.myUsdtWalletAddress, "Unauthorized jetton sender");

        let payload: Slice = msg.forwardPayload;
        let tenantId: Int = payload.loadUint(128);
        let itemId: Int = payload.loadUint(32);

        let hasReferrer: Bool = payload.loadBool();
        let referrerAddress: Address? = null;
        let referrerFeeBps: Int = 0;
        if (hasReferrer) {
            referrerAddress = payload.loadAddress();
            referrerFeeBps = payload.loadUint(16);
        }

        let hasSupervisor: Bool = payload.loadBool();
        let supervisorAddress: Address? = null;
        let supervisorFeeBps: Int = 0;
        if (hasSupervisor) {
            supervisorAddress = payload.loadAddress();
            supervisorFeeBps = payload.loadUint(16);
        }

        let tenant: TenantConfig? = self.tenants.get(tenantId);
        require(tenant != null, "Tenant not found");
        require(tenant!!.active, "Tenant not active");

        let sumBps: Int = self.platformFeeBps + referrerFeeBps + supervisorFeeBps;
        require(sumBps <= 10000, "BPS overflow");

        let totalAmount: Int = msg.amount;

        // Step 1: Platform
        let platformAmount: Int = totalAmount * self.platformFeeBps / 10000;
        self._sendJetton(self.platformAddress, platformAmount);
        let remainder: Int = totalAmount - platformAmount;

        // Step 2: Referrer
        if (referrerAddress != null && referrerFeeBps > 0 && remainder > 0) {
            let referrerAmount: Int = totalAmount * referrerFeeBps / 10000;
            self._sendJetton(referrerAddress!!, referrerAmount);
            remainder = remainder - referrerAmount;
        }

        // Step 3: Supervisor
        if (supervisorAddress != null && supervisorFeeBps > 0 && remainder > 0) {
            let supervisorAmount: Int = totalAmount * supervisorFeeBps / 10000;
            self._sendJetton(supervisorAddress!!, supervisorAmount);
            remainder = remainder - supervisorAmount;
        }

        // Step 4: Tenant gets remainder
        if (remainder > 1000) {
            self._sendJetton(tenant!!.payoutAddress, remainder);
        }

        emit(PaymentProcessed{
            tenantId: tenantId,
            itemId: itemId,
            payer: msg.sender,
            amount: totalAmount,
            currency: 1
        }.toCell());
    }

    // ─── _sendJetton (internal) ──────────────────────────────────────────────

    fun _sendJetton(to: Address, amount: Int) {
        let transferBody: Builder = beginCell()
            .storeUint(0xf8a7ea5, 32)    // op: jetton transfer (TEP-74)
            .storeUint(0, 64)             // query_id
            .storeCoins(amount)           // amount
            .storeAddress(to)             // destination
            .storeAddress(myAddress())    // response_destination
            .storeBit(false)              // no custom payload
            .storeCoins(0)               // forward_amount = 0
            .storeBit(false);             // no forward payload

        send(SendParameters{
            to: self.myUsdtWalletAddress,
            value: ton("0.012"),
            mode: SendIgnoreErrors,
            body: transferBody.endCell()
        });
    }

    // ─── UpdateTenant ────────────────────────────────────────────────────────

    receive(msg: UpdateTenant) {
        require(self.active, "Contract is paused");
        let tenant: TenantConfig? = self.tenants.get(msg.tenantId);
        require(tenant != null, "Tenant not found");
        require(tenant!!.ownerAddress == sender(), "Not tenant owner");

        self.tenants.set(msg.tenantId, TenantConfig{
            ownerAddress: tenant!!.ownerAddress,
            payoutAddress: msg.newPayoutAddress,
            active: msg.active
        });
    }

    // ─── Admin: withdraw TON ─────────────────────────────────────────────────
    // Выводит накопленную сдачу, оставляя минимум 0.5 TON на контракте
    // для оплаты Storage Fee (аренды ячеек)

    receive("withdraw") {
        require(sender() == self.owner, "Only owner");
        let reserve: Int = ton("0.5");
        let available: Int = myBalance() - reserve;
        require(available > ton("0.01"), "Nothing to withdraw");
        send(SendParameters{
            to: self.owner,
            bounce: false,
            value: available,
            mode: SendIgnoreErrors
        });
    }

    // ─── Admin: withdraw stuck USDT ──────────────────────────────────────────

    receive(msg: WithdrawJetton) {
        require(sender() == self.owner, "Only owner");
        self._sendJetton(self.owner, msg.amount);
    }

    // ─── AdminUpdate ─────────────────────────────────────────────────────────

    receive(msg: AdminUpdate) {
        require(sender() == self.owner, "Only owner");
        if (msg.platformAddress != null) {
            self.platformAddress = msg.platformAddress!!;
        }
        if (msg.platformFeeBps != null) {
            require(msg.platformFeeBps!! <= 5000, "Max 50%");
            self.platformFeeBps = msg.platformFeeBps!!;
        }
        if (msg.usdtMasterAddress != null) {
            self.usdtMasterAddress = msg.usdtMasterAddress!!;
        }
        if (msg.usdtWalletAddress != null) {
            self.myUsdtWalletAddress = msg.usdtWalletAddress!!;
        }
        if (msg.active != null) {
            self.active = msg.active!!;
        }
        emit(AdminUpdated{}.toCell());
    }

    // ─── Getters ─────────────────────────────────────────────────────────────

    get fun getTenant(tenantId: Int): TenantConfig? {
        return self.tenants.get(tenantId);
    }

    get fun getItem(tenantId: Int, itemId: Int): ItemConfig? {
        let itemKey: Int = (tenantId << 32) | itemId;
        return self.items.get(itemKey);
    }

    get fun getPlatformConfig(): PlatformInfo {
        return PlatformInfo{
            owner: self.owner,
            platformAddress: self.platformAddress,
            platformFeeBps: self.platformFeeBps,
            usdtMasterAddress: self.usdtMasterAddress,
            myUsdtWalletAddress: self.myUsdtWalletAddress,
            active: self.active
        };
    }
}

struct PlatformInfo {
    owner: Address;
    platformAddress: Address;
    platformFeeBps: Int;
    usdtMasterAddress: Address;
    myUsdtWalletAddress: Address;
    active: Bool;
}
