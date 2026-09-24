use super::kpi;

pub struct Export(pub &'static str, pub *const ());

unsafe impl Sync for Export {}

macro_rules! exports {
    ($($name:literal => $value:path),* $(,)?) => {
        pub static TABLE: &[Export] = &[
            $(Export($name, $value as *const ())),*
        ];
    };
}

exports! {
    "hamix_kpi_version" => kpi::hamix_kpi_version,
    "hamix_kmalloc" => kpi::hamix_kmalloc,
    "hamix_kzalloc" => kpi::hamix_kzalloc,
    "hamix_kfree" => kpi::hamix_kfree,
    "hamix_dma_alloc" => kpi::hamix_dma_alloc,
    "hamix_dma_free" => kpi::hamix_dma_free,
    "hamix_virt_to_phys" => kpi::hamix_virt_to_phys,
    "hamix_phys_to_virt" => kpi::hamix_phys_to_virt,
    "hamix_ioremap" => kpi::hamix_ioremap,
    "hamix_iounmap" => kpi::hamix_iounmap,
    "hamix_readb" => kpi::hamix_readb,
    "hamix_readw" => kpi::hamix_readw,
    "hamix_readl" => kpi::hamix_readl,
    "hamix_readq" => kpi::hamix_readq,
    "hamix_writeb" => kpi::hamix_writeb,
    "hamix_writew" => kpi::hamix_writew,
    "hamix_writel" => kpi::hamix_writel,
    "hamix_writeq" => kpi::hamix_writeq,
    "hamix_inb" => kpi::hamix_inb,
    "hamix_inw" => kpi::hamix_inw,
    "hamix_inl" => kpi::hamix_inl,
    "hamix_outb" => kpi::hamix_outb,
    "hamix_outw" => kpi::hamix_outw,
    "hamix_outl" => kpi::hamix_outl,
    "hamix_pci_find" => kpi::hamix_pci_find,
    "hamix_request_irq" => kpi::hamix_request_irq,
    "hamix_free_irq" => kpi::hamix_free_irq,
    "hamix_pci_msi_enable" => kpi::hamix_pci_msi_enable,
    "hamix_schedule_work" => kpi::hamix_schedule_work,
    "hamix_in_interrupt" => kpi::hamix_in_interrupt,
    "hamix_param_u32" => kpi::hamix_param_u32,
    "hamix_dev_log" => kpi::hamix_dev_log,
    "hamix_pci_read32" => kpi::hamix_pci_read32,
    "hamix_pci_write32" => kpi::hamix_pci_write32,
    "hamix_pci_read16" => kpi::hamix_pci_read16,
    "hamix_pci_write16" => kpi::hamix_pci_write16,
    "hamix_pci_bar" => kpi::hamix_pci_bar,
    "hamix_pci_enable" => kpi::hamix_pci_enable,
    "hamix_udelay" => kpi::hamix_udelay,
    "hamix_mdelay" => kpi::hamix_mdelay,
    "hamix_uptime_ms" => kpi::hamix_uptime_ms,
    "hamix_printk" => kpi::hamix_printk,
    "hamix_claim_device" => kpi::hamix_claim_device,
    "hamix_register_display" => kpi::hamix_register_display,
    "hamix_display_hotplug" => kpi::hamix_display_hotplug,
    "hamix_pci_find_class" => super::classes::hamix_pci_find_class,
    "hamix_device_from_pci" => super::classes::hamix_device_from_pci,
    "hamix_platform_find" => super::classes::hamix_platform_find,
    "hamix_dma_run" => super::classes::hamix_dma_run,
    "hamix_register_block" => super::classes::hamix_register_block,
    "hamix_input_key" => super::classes::hamix_input_key,
    "hamix_input_pointer" => super::classes::hamix_input_pointer,
    "hamix_input_absolute" => super::classes::hamix_input_absolute,
    "hamix_register_audio" => super::classes::hamix_register_audio,
    "hamix_bus_publish" => super::classes::hamix_bus_publish,
    "hamix_usb_register_hcd" => super::classes::hamix_usb_register_hcd,
    "hamix_usb_add_device" => super::classes::hamix_usb_add_device,
    "hamix_usb_remove_device" => super::classes::hamix_usb_remove_device,
    "hamix_usb_hid_report" => super::classes::hamix_usb_hid_report,
    "hamix_register_netdev" => super::classes::hamix_register_netdev,
    "hamix_unregister_netdev" => super::classes::hamix_unregister_netdev,
    "hamix_net_receive" => super::classes::hamix_net_receive,
    "hamix_net_carrier" => super::classes::hamix_net_carrier,
    "hamix_edid_modes" => kpi::hamix_edid_modes,
    "hamix_display_abi" => kpi::hamix_display_abi,
    "hamix_display_changed" => kpi::hamix_display_changed,
    "hamix_display_current" => kpi::hamix_display_current,
    "hamix_display_describe" => kpi::hamix_display_describe,
    "hamix_budget_left" => kpi::hamix_budget_left,
    "memcpy" => builtin_memcpy,
    "memset" => builtin_memset,
    "memmove" => builtin_memmove,
    "memcmp" => builtin_memcmp,
}

unsafe extern "C" {
    #[link_name = "memcpy"]
    fn builtin_memcpy(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    #[link_name = "memset"]
    fn builtin_memset(dest: *mut u8, value: i32, n: usize) -> *mut u8;
    #[link_name = "memmove"]
    fn builtin_memmove(dest: *mut u8, src: *const u8, n: usize) -> *mut u8;
    #[link_name = "memcmp"]
    fn builtin_memcmp(a: *const u8, b: *const u8, n: usize) -> i32;
}

extern "C" fn module_abort() -> ! {
    crate::drivers::klog::log("module: a loaded module hit a panic path and was stopped");
    loop {
        crate::arch::hlt();
    }
}

fn is_bailout(name: &str) -> bool {
    if name == "rust_begin_unwind" || name == "__stack_chk_fail" || name.contains("panic") || name.contains("unwind") {
        return true;
    }
    name.starts_with("_RNv") && name.contains("4core") && (name.ends_with("_fail") || name.contains("panicking"))
}

pub fn lookup(name: &str) -> Option<u64> {
    if let Some(entry) = TABLE.iter().find(|e| e.0 == name) {
        return Some(entry.1 as u64);
    }
    if is_bailout(name) {
        return Some(module_abort as *const () as u64);
    }
    None
}

pub fn count() -> usize {
    TABLE.len()
}

pub fn for_each<F: FnMut(&str)>(mut f: F) {
    for entry in TABLE {
        f(entry.0);
    }
}
