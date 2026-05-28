#![allow(clippy::doc_markdown, clippy::needless_raw_string_hashes)]

//! Visual gallery of fabricated smali snippets rendered through
//! `diff::render_smali_diff`, for eyeballing the highlighter. Asserts
//! nothing; the snippets exercise patch shapes from `morphe_patches.md`.
//!
//! Run:    `cargo test gallery -- --nocapture`
//! Filter: `FILTER=adguard cargo test gallery -- --nocapture`

use crate::diff::{RenderMode, render_smali_diff};
use crate::output::style;

struct Patch {
    section: &'static str,
    name: &'static str,
    file: &'static str,
    before: &'static str,
    after: &'static str,
}

#[test]
fn gallery() {
    let filter = std::env::var("FILTER").ok();
    let needle = filter.as_deref().map(str::to_ascii_lowercase);
    let mut current_section: Option<&str> = None;
    let mut shown = 0usize;

    for patch in PATCHES {
        if let Some(needle) = needle.as_deref() {
            let hay =
                format!("{} {} {}", patch.section, patch.name, patch.file).to_ascii_lowercase();
            if !hay.contains(needle) {
                continue;
            }
        }

        if current_section != Some(patch.section) {
            println!();
            println!(
                "{}",
                style::bold(&style::cyan(&format!("═══ {} ═══", patch.section)))
            );
            current_section = Some(patch.section);
        }

        println!();
        println!("  {} {}", style::cyan("▸"), style::bold(patch.name));
        println!("    {}", style::dimmed(&format!("file: {}", patch.file)));
        println!();

        let rendered = render_smali_diff(patch.before, patch.after, 3, RenderMode::Human);
        for line in rendered.lines() {
            println!("    {line}");
        }

        shown += 1;
    }

    println!();
    if shown == 0 {
        println!("{}", style::dimmed("No patches matched."));
    } else {
        println!("{}", style::dimmed(&format!("{shown} patch(es) shown")));
    }
}

const PATCHES: &[Patch] = &[
    Patch {
        section: "AdGuard",
        name: "Enable Premium — redirect getPlusState to getPaidLicense",
        file: "GetPlusStateFingerprint",
        before: r#".method public getPlusState()Lcom/adguard/PlusState;
    .registers 3
    iget-object v0, p0, Lcom/adguard/StateManager;->state:Lcom/adguard/PlusState;
    if-nez v0, :cond_0
    sget-object v0, Lcom/adguard/PlusState;->FREE:Lcom/adguard/PlusState;
    :cond_0
    return-object v0
.end method"#,
        after: r#".method public getPlusState()Lcom/adguard/PlusState;
    .registers 3
    invoke-static {}, Lcom/adguard/StateManager;->getPaidLicense()Lcom/adguard/PlusState;
    move-result-object v0
    return-object v0
.end method"#,
    },
    Patch {
        section: "All (Universal)",
        name: "Disable Pairip — neutralize processLicenseResponse",
        file: "ProcessLicenseResponseFingerprint",
        before: r#".method public processLicenseResponse(I)V
    .registers 4
    if-eqz p1, :cond_0
    invoke-direct {p0, p1}, Lcom/pairip/Licensing;->reportFailure(I)V
    :cond_0
    return-void
.end method"#,
        after: r#".method public processLicenseResponse(I)V
    .registers 4
    const/4 p1, 0x0
    if-eqz p1, :cond_0
    invoke-direct {p0, p1}, Lcom/pairip/Licensing;->reportFailure(I)V
    :cond_0
    return-void
.end method"#,
    },
    Patch {
        section: "All (Universal)",
        name: "Disable Pairip — neutralize validateLicenseResponse",
        file: "ValidateLicenseResponseFingerprint",
        before: r#".method public validateLicenseResponse()V
    .registers 4
    iget-object v0, p0, Lcom/pairip/Licensing;->state:Lcom/pairip/State;
    invoke-virtual {v0}, Lcom/pairip/State;->ensureValid()V
    return-void
.end method"#,
        after: r#".method public validateLicenseResponse()V
    .registers 4
    return-void
    iget-object v0, p0, Lcom/pairip/Licensing;->state:Lcom/pairip/State;
    invoke-virtual {v0}, Lcom/pairip/State;->ensureValid()V
    return-void
.end method"#,
    },
    Patch {
        section: "AllTrails",
        name: "Enable Peak — isPro returns true",
        file: "IsProFingerprint",
        before: r#".method public isPro()Z
    .registers 2
    iget-boolean v0, p0, Lcom/alltrails/UserState;->subscriptionActive:Z
    return v0
.end method"#,
        after: r#".method public isPro()Z
    .registers 2
    const/4 v0, 0x1
    return v0
.end method"#,
    },
    Patch {
        section: "AllTrails",
        name: "Enable Peak — getSubscriptionTier returns \"peak\"",
        file: "GetSubscriptionTierFingerprint",
        before: r#".method public getSubscriptionTier()Ljava/lang/String;
    .registers 2
    iget-object v0, p0, Lcom/alltrails/UserState;->tier:Ljava/lang/String;
    return-object v0
.end method"#,
        after: r#".method public getSubscriptionTier()Ljava/lang/String;
    .registers 2
    const-string v0, "peak"
    return-object v0
.end method"#,
    },
    Patch {
        section: "Avocards",
        name: "Mark UserEntity.premium = true before return",
        file: "UserEntityCtorFingerprint",
        before: r#".method public constructor <init>(Ljava/lang/String;)V
    .registers 4
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    iput-object p1, p0, Lcom/avocards/UserEntity;->id:Ljava/lang/String;
    return-void
.end method"#,
        after: r#".method public constructor <init>(Ljava/lang/String;)V
    .registers 4
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    iput-object p1, p0, Lcom/avocards/UserEntity;->id:Ljava/lang/String;
    const/4 v1, 0x1
    iput-boolean v1, p0, Lcom/avocards/UserEntity;->premium:Z
    return-void
.end method"#,
    },
    Patch {
        section: "Avocards",
        name: "Replace default Boolean load with TRUE",
        file: "GetPremiumBaseFingerprint",
        before: r#".method public getPremiumBase()Ljava/lang/Boolean;
    .registers 2
    sget-object v0, Ljava/lang/Boolean;->FALSE:Ljava/lang/Boolean;
    return-object v0
.end method"#,
        after: r#".method public getPremiumBase()Ljava/lang/Boolean;
    .registers 2
    sget-object v0, Ljava/lang/Boolean;->TRUE:Ljava/lang/Boolean;
    return-object v0
.end method"#,
    },
    Patch {
        section: "Busuu",
        name: "Tier returns \"standard\"",
        file: "GetTierFingerprint",
        before: r#".method public getTier()Ljava/lang/String;
    .registers 3
    iget-object v0, p0, Lcom/busuu/Account;->tier:Ljava/lang/String;
    if-nez v0, :cond_0
    const-string v0, "free"
    :cond_0
    return-object v0
.end method"#,
        after: r#".method public getTier()Ljava/lang/String;
    .registers 3
    const-string v0, "standard"
    return-object v0
.end method"#,
    },
    Patch {
        section: "CamScanner",
        name: "Status code returns 1L",
        file: "GetStatusCodeFingerprint",
        before: r#".method public getStatusCode()J
    .registers 3
    iget-wide v0, p0, Lcom/intsig/CamScanner;->statusCode:J
    return-wide v0
.end method"#,
        after: r#".method public getStatusCode()J
    .registers 3
    const-wide/16 v0, 0x1
    return-wide v0
.end method"#,
    },
    Patch {
        section: "Duolingo",
        name: "Force debug mode in BuildConfigProvider",
        file: "BuildConfigProviderCtor",
        before: r#".method public constructor <init>()V
    .registers 2
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    const/4 v0, 0x0
    iput-boolean v0, p0, Lcom/duolingo/BuildConfigProvider;->isDebug:Z
    return-void
.end method"#,
        after: r#".method public constructor <init>()V
    .registers 2
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    const/4 v0, 0x0
    iput-boolean v0, p0, Lcom/duolingo/BuildConfigProvider;->isDebug:Z
    const/4 v0, 0x1
    iput-boolean v0, p0, Lcom/duolingo/BuildConfigProvider;->isDebug:Z
    return-void
.end method"#,
    },
    Patch {
        section: "Duolingo",
        name: "Force User flags in LoggedInState constructor",
        file: "LoggedInStateCtorFingerprint",
        before: r#".method public constructor <init>(Lcom/duolingo/User;)V
    .registers 3
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    iput-object p1, p0, Lcom/duolingo/LoggedInState;->user:Lcom/duolingo/User;
    return-void
.end method"#,
        after: r#".method public constructor <init>(Lcom/duolingo/User;)V
    .registers 3
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    iput-object p1, p0, Lcom/duolingo/LoggedInState;->user:Lcom/duolingo/User;
    const/4 v0, 0x1
    iput-boolean v0, p1, Lcom/duolingo/User;->isPaid:Z
    iput-boolean v0, p1, Lcom/duolingo/User;->hasPlus:Z
    sget-object v0, Lcom/duolingo/SubscriberLevel;->GOLD:Lcom/duolingo/SubscriberLevel;
    iput-object v0, p1, Lcom/duolingo/User;->subscriberLevel:Lcom/duolingo/SubscriberLevel;
    return-void
.end method"#,
    },
    Patch {
        section: "FotMob",
        name: "Inject entitlement at check-cast site",
        file: "EntitlementCheckCastSite",
        before: r#".method public buildEntitlement(Ljava/lang/Object;)Lcom/fotmob/Entitlement;
    .registers 4
    check-cast p1, Lcom/fotmob/Entitlement;
    return-object p1
.end method"#,
        after: r#".method public buildEntitlement(Ljava/lang/Object;)Lcom/fotmob/Entitlement;
    .registers 4
    const-string v0, "FOTMOB_PLUS"
    invoke-static {v0}, Lhoodles/fotmob/EnablePlusPatch;->createEntitlement(Ljava/lang/String;)Ljava/lang/Object;
    move-result-object p1
    check-cast p1, Lcom/fotmob/Entitlement;
    return-object p1
.end method"#,
    },
    Patch {
        section: "GitHub",
        name: "AMOLED — force black surface colors in FunctionalColors",
        file: "FunctionalColorsCtorFingerprint",
        before: r#".method constructor <init>(JJ)V
    .registers 7
    iput-wide p1, p0, Lcom/github/FunctionalColors;->background:J
    iput-wide p3, p0, Lcom/github/FunctionalColors;->surface:J
    return-void
.end method"#,
        after: r#".method constructor <init>(JJ)V
    .registers 7
    const-wide p1, 0xff000000000000L
    const-wide p3, 0xff000000000000L
    iput-wide p1, p0, Lcom/github/FunctionalColors;->background:J
    iput-wide p3, p0, Lcom/github/FunctionalColors;->surface:J
    return-void
.end method"#,
    },
    Patch {
        section: "Icon Packer",
        name: "Force premium flag in PurchaseUiState",
        file: "PurchaseUiStateCtorFingerprint",
        before: r#".method public constructor <init>()V
    .registers 2
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    const/4 v0, 0x0
    iput-boolean v0, p0, Lcom/iconpacker/PurchaseUiState;->isPremium:Z
    return-void
.end method"#,
        after: r#".method public constructor <init>()V
    .registers 2
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    const/4 v0, 0x1
    iput-boolean v0, p0, Lcom/iconpacker/PurchaseUiState;->isPremium:Z
    return-void
.end method"#,
    },
    Patch {
        section: "MacroFactor",
        name: "Replace signature getter with hardcoded byte array",
        file: "GetSignatureFingerprint",
        before: r#".method public getSignature()[B
    .registers 3
    invoke-virtual {p0}, Lcom/macrofactor/App;->getPackageManager()Landroid/content/pm/PackageManager;
    move-result-object v0
    invoke-virtual {v0}, Landroid/content/pm/PackageManager;->getSignatureBytes()[B
    move-result-object v0
    return-object v0
.end method"#,
        after: r#".method public getSignature()[B
    .registers 3
    const/16 v0, 0x14
    new-array v0, v0, [B
    fill-array-data v0, :array_sig
    return-object v0
    :array_sig
    .array-data 1
        0x30t 0x82t 0x02t 0x5at 0x30t 0x82t 0x01t 0x42t
    .end array-data
.end method"#,
    },
    Patch {
        section: "Meme Generator",
        name: "checkLicenseMillis returns Long.MAX_VALUE",
        file: "CheckLicenseMillis",
        before: r#".method public checkLicenseMillis()J
    .registers 4
    iget-wide v0, p0, Lcom/memes/License;->expiresAtMillis:J
    return-wide v0
.end method"#,
        after: r#".method public checkLicenseMillis()J
    .registers 4
    const-wide v0, 0x7fffffffffffffffL
    return-wide v0
.end method"#,
    },
    Patch {
        section: "Mirinae",
        name: "Inject JS-patching shouldInterceptRequest",
        file: "ShouldInterceptRequestFingerprint",
        before: r#".method public shouldInterceptRequest(Landroid/webkit/WebView;Landroid/webkit/WebResourceRequest;)Landroid/webkit/WebResourceResponse;
    .registers 5
    invoke-super {p0, p1, p2}, Landroid/webkit/WebViewClient;->shouldInterceptRequest(Landroid/webkit/WebView;Landroid/webkit/WebResourceRequest;)Landroid/webkit/WebResourceResponse;
    move-result-object v0
    return-object v0
.end method"#,
        after: r#".method public shouldInterceptRequest(Landroid/webkit/WebView;Landroid/webkit/WebResourceRequest;)Landroid/webkit/WebResourceResponse;
    .registers 5
    invoke-static {p2}, Lhoodles/mirinae/EnableProPatch;->patchAppJavascript(Landroid/webkit/WebResourceRequest;)Landroid/webkit/WebResourceResponse;
    move-result-object v0
    if-eqz v0, :cond_default
    return-object v0
    :cond_default
    invoke-super {p0, p1, p2}, Landroid/webkit/WebViewClient;->shouldInterceptRequest(Landroid/webkit/WebView;Landroid/webkit/WebResourceRequest;)Landroid/webkit/WebResourceResponse;
    move-result-object v0
    return-object v0
.end method"#,
    },
    Patch {
        section: "MyExpenses",
        name: "Force PROFESSIONAL on setLicenseStatus",
        file: "SetLicenseStatusFingerprint",
        before: r#".method public setLicenseStatus(Lcom/myexpenses/LicenceStatus;)V
    .registers 3
    iput-object p1, p0, Lcom/myexpenses/LicenseManager;->status:Lcom/myexpenses/LicenceStatus;
    return-void
.end method"#,
        after: r#".method public setLicenseStatus(Lcom/myexpenses/LicenceStatus;)V
    .registers 3
    sget-object p1, Lcom/myexpenses/LicenceStatus;->PROFESSIONAL:Lcom/myexpenses/LicenceStatus;
    iput-object p1, p0, Lcom/myexpenses/LicenseManager;->status:Lcom/myexpenses/LicenceStatus;
    return-void
.end method"#,
    },
    Patch {
        section: "Niagara Launcher",
        name: "Force pro params in ProState constructor",
        file: "ProStateConstructorFingerprint",
        before: r#".method public constructor <init>(ZZ)V
    .registers 4
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    iput-boolean p1, p0, Lcom/niagara/ProState;->isPro:Z
    iput-boolean p2, p0, Lcom/niagara/ProState;->hasTrial:Z
    return-void
.end method"#,
        after: r#".method public constructor <init>(ZZ)V
    .registers 4
    const/4 p1, 0x1
    const/4 p2, 0x1
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    iput-boolean p1, p0, Lcom/niagara/ProState;->isPro:Z
    iput-boolean p2, p0, Lcom/niagara/ProState;->hasTrial:Z
    return-void
.end method"#,
    },
    Patch {
        section: "NOMone Desktop",
        name: "Bump trial PurchaseInfo.status to 4",
        file: "GetPurchaseInfoFingerprint",
        before: r#".method public getPurchaseInfo()Lcom/nomone/PurchaseInfo;
    .registers 3
    new-instance v0, Lcom/nomone/PurchaseInfo;
    invoke-direct {v0}, Lcom/nomone/PurchaseInfo;-><init>()V
    return-object v0
.end method"#,
        after: r#".method public getPurchaseInfo()Lcom/nomone/PurchaseInfo;
    .registers 3
    new-instance v0, Lcom/nomone/PurchaseInfo;
    invoke-direct {v0}, Lcom/nomone/PurchaseInfo;-><init>()V
    const/4 v1, 0x4
    iput v1, v0, Lcom/nomone/PurchaseInfo;->status:I
    return-object v0
.end method"#,
    },
    Patch {
        section: "Nova Launcher",
        name: "Force prime bitmask after preference read",
        file: "SetPrimeFromPreferencesFingerprint",
        before: r#".method public setPrimeFromPreferences(Landroid/content/SharedPreferences;)V
    .registers 4
    const-string v0, "prime"
    const/4 v1, 0x0
    invoke-interface {p1, v0, v1}, Landroid/content/SharedPreferences;->getInt(Ljava/lang/String;I)I
    move-result v0
    iput v0, p0, Lcom/teslacoilsw/Launcher;->primeFlags:I
    return-void
.end method"#,
        after: r#".method public setPrimeFromPreferences(Landroid/content/SharedPreferences;)V
    .registers 4
    const-string v0, "prime"
    const/4 v1, 0x0
    invoke-interface {p1, v0, v1}, Landroid/content/SharedPreferences;->getInt(Ljava/lang/String;I)I
    move-result v0
    const/16 v0, 0x200
    iput v0, p0, Lcom/teslacoilsw/Launcher;->primeFlags:I
    return-void
.end method"#,
    },
    Patch {
        section: "Prime Video",
        name: "Inject skip-ads handler after getPrimaryPlayer()",
        file: "EnterServerInsertedAdBreakStateFingerprint",
        before: r#".method enterServerInsertedAdBreakState(Lcom/amazon/AdBreak;)V
    .registers 4
    invoke-virtual {p0}, Lcom/amazon/AdsPlayer;->getPrimaryPlayer()Lcom/amazon/Player;
    move-result-object v0
    invoke-virtual {v0, p1}, Lcom/amazon/Player;->enterAdBreak(Lcom/amazon/AdBreak;)V
    return-void
.end method"#,
        after: r#".method enterServerInsertedAdBreakState(Lcom/amazon/AdBreak;)V
    .registers 4
    invoke-virtual {p0}, Lcom/amazon/AdsPlayer;->getPrimaryPlayer()Lcom/amazon/Player;
    move-result-object v0
    invoke-static {p0, p1, v0}, Lhoodles/primevideo/SkipAdsPatch;->enterServerInsertedAdBreakState(Lcom/amazon/AdsPlayer;Lcom/amazon/AdBreak;Lcom/amazon/Player;)V
    return-void
.end method"#,
    },
    Patch {
        section: "Proton VPN",
        name: "Custom DNS — clear isRestricted flag",
        file: "CustomDNSSettingViewStateCtor",
        before: r#".method public constructor <init>(Lcom/protonvpn/Plan;)V
    .registers 3
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    invoke-virtual {p1}, Lcom/protonvpn/Plan;->isPlus()Z
    move-result v0
    if-nez v0, :cond_unlocked
    const/4 v0, 0x1
    goto :store
    :cond_unlocked
    const/4 v0, 0x0
    :store
    iput-boolean v0, p0, Lcom/protonvpn/CustomDnsView;->isRestricted:Z
    return-void
.end method"#,
        after: r#".method public constructor <init>(Lcom/protonvpn/Plan;)V
    .registers 3
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    const/4 v0, 0x0
    iput-boolean v0, p0, Lcom/protonvpn/CustomDnsView;->isRestricted:Z
    return-void
.end method"#,
    },
    Patch {
        section: "PyDroid3",
        name: "Force premium parameter at top of setIsPremium",
        file: "SetIsPremiumFingerprint",
        before: r#".method public setIsPremium(Z)V
    .registers 3
    iput-boolean p1, p0, Lcom/pydroid/PremiumState;->isPremium:Z
    return-void
.end method"#,
        after: r#".method public setIsPremium(Z)V
    .registers 3
    const/4 p1, 0x1
    iput-boolean p1, p0, Lcom/pydroid/PremiumState;->isPremium:Z
    return-void
.end method"#,
    },
    Patch {
        section: "Smart Launcher",
        name: "Auto-grant lifetime purchase if ads enabled",
        file: "PurchaseItemsCtor",
        before: r#".method public constructor <init>()V
    .registers 4
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    return-void
.end method"#,
        after: r#".method public constructor <init>()V
    .registers 4
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    sget-object v0, Lcom/smartlauncher/PurchaseItems;->lifetime:Lcom/smartlauncher/PurchasableItem;
    invoke-virtual {v0}, Lcom/smartlauncher/PurchasableItem;->get()Z
    move-result v1
    if-nez v1, :end_init
    invoke-static {}, Lcom/smartlauncher/GetApp;->get()Landroid/content/Context;
    move-result-object v1
    const/4 v2, 0x1
    invoke-virtual {v0, v1, v2}, Lcom/smartlauncher/PurchasableItem;->set(Landroid/content/Context;Z)V
    :end_init
    return-void
.end method"#,
    },
    Patch {
        section: "Solid Explorer",
        name: "Auto-upgrade to PREMIUM_PRO if ads enabled",
        file: "LicenseDetailsCtorFingerprint",
        before: r#".method public constructor <init>()V
    .registers 4
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    sget-object v0, Lcom/solidexplorer/LicenseState;->FREE:Lcom/solidexplorer/LicenseState;
    iput-object v0, p0, Lcom/solidexplorer/LicenseDetails;->a:Lcom/solidexplorer/LicenseState;
    return-void
.end method"#,
        after: r#".method public constructor <init>()V
    .registers 4
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    sget-object v0, Lcom/solidexplorer/LicenseState;->FREE:Lcom/solidexplorer/LicenseState;
    iput-object v0, p0, Lcom/solidexplorer/LicenseDetails;->a:Lcom/solidexplorer/LicenseState;
    invoke-virtual {p0}, Lcom/solidexplorer/LicenseDetails;->adsEnabled()Z
    move-result v0
    if-eqz v0, :end_init
    sget-object v0, Lcom/solidexplorer/LicenseState;->PREMIUM_PRO:Lcom/solidexplorer/LicenseState;
    iput-object v0, p0, Lcom/solidexplorer/LicenseDetails;->a:Lcom/solidexplorer/LicenseState;
    :end_init
    return-void
.end method"#,
    },
    Patch {
        section: "SoundCloud",
        name: "Spoof UserConsumerPlan params to Go+",
        file: "UserConsumerPlanConstructorFingerprint",
        before: r#".method public constructor <init>(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V
    .registers 7
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    iput-object p1, p0, Lcom/soundcloud/UserConsumerPlan;->tier:Ljava/lang/String;
    iput-object p5, p0, Lcom/soundcloud/UserConsumerPlan;->planId:Ljava/lang/String;
    iput-object p6, p0, Lcom/soundcloud/UserConsumerPlan;->displayName:Ljava/lang/String;
    return-void
.end method"#,
        after: r#".method public constructor <init>(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V
    .registers 7
    const-string p1, "high_tier"
    const-string p5, "go-plus"
    const-string p6, "SoundCloud Go"
    invoke-direct {p0}, Ljava/lang/Object;-><init>()V
    iput-object p1, p0, Lcom/soundcloud/UserConsumerPlan;->tier:Ljava/lang/String;
    iput-object p5, p0, Lcom/soundcloud/UserConsumerPlan;->planId:Ljava/lang/String;
    iput-object p6, p0, Lcom/soundcloud/UserConsumerPlan;->displayName:Ljava/lang/String;
    return-void
.end method"#,
    },
    Patch {
        section: "SoundCloud",
        name: "Force HIGH tier in getDowngradeTier",
        file: "GetDowngradeTierFingerprint",
        before: r#".method public getDowngradeTier()Lcom/soundcloud/Tier;
    .registers 2
    iget-object v0, p0, Lcom/soundcloud/Account;->downgradeTier:Lcom/soundcloud/Tier;
    return-object v0
.end method"#,
        after: r#".method public getDowngradeTier()Lcom/soundcloud/Tier;
    .registers 2
    sget-object v0, Lcom/soundcloud/Tier;->HIGH:Lcom/soundcloud/Tier;
    return-object v0
.end method"#,
    },
    Patch {
        section: "Teuida",
        name: "Premium getter returns boxed true",
        file: "PremiumGetterFingerprint",
        before: r#".method public getPremium()Ljava/lang/Boolean;
    .registers 2
    iget-object v0, p0, Lcom/teuida/Account;->premium:Ljava/lang/Boolean;
    return-object v0
.end method"#,
        after: r#".method public getPremium()Ljava/lang/Boolean;
    .registers 2
    sget-object v0, Ljava/lang/Boolean;->TRUE:Ljava/lang/Boolean;
    return-object v0
.end method"#,
    },
    Patch {
        section: "Wallcraft",
        name: "Replace getSubscriptionState with InfiniteSubscription",
        file: "GetSubscriptionStateFingerprint",
        before: r#".method public getSubscriptionState()Lcom/wallcraft/SubscriptionState;
    .registers 3
    iget-object v0, p0, Lcom/wallcraft/Account;->subscription:Lcom/wallcraft/SubscriptionState;
    if-nez v0, :cond_0
    sget-object v0, Lcom/wallcraft/SubscriptionState;->NONE:Lcom/wallcraft/SubscriptionState;
    :cond_0
    return-object v0
.end method"#,
        after: r#".method public getSubscriptionState()Lcom/wallcraft/SubscriptionState;
    .registers 3
    new-instance v0, Lcom/wallcraft/InfiniteSubscription;
    invoke-direct {v0}, Lcom/wallcraft/InfiniteSubscription;-><init>()V
    return-object v0
.end method"#,
    },
    Patch {
        section: "Merriam-Webster",
        name: "Synthesize forever-valid Subscription",
        file: "GetSubscriptionFingerprint",
        before: r#".method public getSubscription()Lcom/merriamwebster/Subscription;
    .registers 4
    iget-object v0, p0, Lcom/merriamwebster/Account;->sub:Lcom/merriamwebster/Subscription;
    return-object v0
.end method"#,
        after: r#".method public getSubscription()Lcom/merriamwebster/Subscription;
    .registers 4
    new-instance v0, Ljava/util/Date;
    const-wide v1, 0x7fffffffffffffffL
    invoke-direct {v0, v1, v2}, Ljava/util/Date;-><init>(J)V
    new-instance v1, Lcom/merriamwebster/Subscription;
    const-string v2, ""
    const/4 v3, 0x1
    invoke-direct {v1, v2, v0, v2, v3}, Lcom/merriamwebster/Subscription;-><init>(Ljava/lang/String;Ljava/util/Date;Ljava/lang/String;Z)V
    return-object v1
.end method"#,
    },
    Patch {
        section: "XRecorder",
        name: "Force pro flag in setDisableAdsUsage",
        file: "SetDisableAdsUsageFingerprint",
        before: r#".method public setDisableAdsUsage(Z)V
    .registers 3
    iget-boolean v0, p0, Lcom/xrecorder/State;->cachedAdsValue:Z
    iput-boolean v0, p0, Lcom/xrecorder/State;->disableAds:Z
    return-void
.end method"#,
        after: r#".method public setDisableAdsUsage(Z)V
    .registers 3
    const/4 v0, 0x1
    iput-boolean v0, p0, Lcom/xrecorder/State;->disableAds:Z
    return-void
.end method"#,
    },
    Patch {
        section: "Shared (SpoofSignaturePatch)",
        name: "Redirect Application <init> to original app class",
        file: "ConstructorFingerprint",
        before: r#".method public constructor <init>()V
    .registers 1
    invoke-direct {p0}, Landroid/app/Application;-><init>()V
    return-void
.end method"#,
        after: r#".method public constructor <init>()V
    .registers 1
    invoke-direct {p0}, Lcom/orig/OriginalApp;-><init>()V
    return-void
.end method"#,
    },
    Patch {
        section: "Shared (SpoofSignaturePatch)",
        name: "Redirect attachBaseContext to original app",
        file: "AttachBaseContextFingerprint",
        before: r#".method protected attachBaseContext(Landroid/content/Context;)V
    .registers 2
    invoke-super {p0, p1}, Landroid/app/Application;->attachBaseContext(Landroid/content/Context;)V
    return-void
.end method"#,
        after: r#".method protected attachBaseContext(Landroid/content/Context;)V
    .registers 2
    invoke-super {p0, p1}, Lcom/orig/OriginalApp;->attachBaseContext(Landroid/content/Context;)V
    return-void
.end method"#,
    },
];
