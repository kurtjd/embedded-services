#![no_std]
#![no_main]

extern crate rt685s_evk_example;

use {defmt_rtt as _, panic_probe as _};

#[embassy_executor::main]
async fn main(_spawner: embassy_executor::Spawner) {
    let _p = embassy_imxrt::init(Default::default());

    use platform_service::nvram;

    #[repr(usize)]
    enum Entries {
        // These sections are unusable:
        //     0,
        //     1,
        //     2,
        General = 3,
        Special = 4,
    }

    static NVRAM_TABLE: nvram::Table<2> = nvram::Table::new(&[Entries::General as usize, Entries::Special as usize]);

    embedded_services::init().await;
    nvram::init(&NVRAM_TABLE).await.unwrap();

    let mut general_section = nvram::lookup_section(NVRAM_TABLE.get_index(Entries::General as usize).unwrap())
        .await
        .unwrap();

    general_section.write(0);
    general_section.write(1);

    use defmt::info;

    info!("general_section = {:?}", general_section.read());

    let special = nvram::lookup_section(NVRAM_TABLE.get_index(Entries::Special as usize).unwrap())
        .await
        .unwrap();
    info!("special = {:?}", special.read());

    let untouchable: Option<nvram::ManagedSection> = nvram::lookup_section(10).await;
    info!("Attempted invalid section is_none = {:?}", untouchable.is_none());
}
