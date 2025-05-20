
package usb_hotplug
import future.keywords.in

# Rule to populate the set of allowed VMs for the input device.
allowed_vms = vms if {
    # Ensure required input fields are present
    input.class
    input.subclass
    input.protocol
    input.vendor_id
    input.product_id

    blacklist = data.usb_hotplug_rules.blacklist
    whitelist = data.usb_hotplug_rules.whitelist
    class_rules = data.usb_hotplug_rules.class_rules
    vm_device_filter = data.usb_hotplug_rules.device_filter
    # Check if the device is blacklisted
    not blacklisted(blacklist, input.vendor_id, input.product_id)
    # Check if the device is mapped to a specific VM
    device_key_0 := sprintf("%s:%s", [input.vendor_id, input.product_id])
    device_key_1 := sprintf("%s:*", [input.vendor_id])
    wl_vms = array.concat(lookup(whitelist, device_key_0), lookup(whitelist, device_key_1))
    
    # Based on class, subclass, and protocol find list VMs which can access it 
    class_key_0 := sprintf("%s:%s:%s", [input.class, input.subclass, input.protocol])
    class_key_1 := sprintf("%s:%s:*", [input.class, input.subclass])
    class_key_2 := sprintf("%s:*:%s", [input.class, input.protocol])
    class_key_3 := sprintf("%s:*:*", [input.class])
    cl_01_vms = array.concat(lookup(class_rules, class_key_0), lookup(class_rules, class_key_1))
    cl_23_vms = array.concat(lookup(class_rules, class_key_2), lookup(class_rules, class_key_3))
	  cl_vms = array.concat(cl_01_vms, cl_23_vms)

    # Merge VMs from all above rules
	  arr_vms = array.concat(wl_vms, cl_vms)
    unique_vms_set := {element | element := arr_vms[_]}
    
    # Filter any VM if it is disabled by the VM
    vms = filter_vms(vm_device_filter, unique_vms_set, device_key_0, device_key_1)
} else = []


lookup(whitelist, key) = vms if {
    some vm_list in [whitelist[key]]
    vms = vm_list
} else = []

whitelisted_in_blacklist(blacklist, vendor_id, product_id) = true if {
    neg_vendor := sprintf("~%s", [vendor_id])
    vendor_whitelist = lookup(blacklist, neg_vendor)
    product_id == vendor_whitelist[_]
}

blacklisted(blacklist, vendor_id, product_id) = true if {
    vendor_blacklist = lookup(blacklist, vendor_id)
    vendor_blacklist[_] == product_id
} else = false if {
    neg_vendor := sprintf("~%s", [vendor_id])
    vendor_whitelist = lookup(blacklist, neg_vendor)
    count(vendor_whitelist) == 0
} else = false if {
    neg_vendor := sprintf("~%s", [vendor_id])
    vendor_whitelist = lookup(blacklist, neg_vendor)
    product_id == vendor_whitelist[_]
} else = true 

is_vm_filtered(vm_device_filter, vm, device_key_0, device_key_1) = true if {
    devices = vm_device_filter[vm]
    device_key_0 == devices[_]
} else = true if {
	devices = vm_device_filter[vm]
    device_key_1 == devices[_]
} else = false

filter_vms(vm_device_filter, sorted_vms, key0, key1) = filtered_vms if {
    filtered_vms := [
        vm |
        some vm in sorted_vms
        not is_vm_filtered(vm_device_filter, vm, key0, key1)
    ]
}
