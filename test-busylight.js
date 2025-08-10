const BusyLight = require('@pureit/busylight').BusyLight;

(async () => {
  try {
    const devices = BusyLight.devices();
    console.log('Devices:', devices);
    if (!devices || devices.length === 0) {
      console.log('No Busylight found!');
      return;
    }
    const busylight = new BusyLight(devices[0]);
    busylight.connect();
    busylight.light('ff0000'); // Turn on red light
    console.log('Busylight set to red.');
  } catch (err) {
    console.error('Error:', err);
  }
})();
