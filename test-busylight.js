const busylightModule = require('./lib');

(async () => {
  try {
    const busylight = busylightModule.get();
    console.log('Busylight initialized. Turning light red...');
    busylight.light('ff0000'); // Turn on red light

    setTimeout(() => {
      console.log('Turning light off.');
      busylight.off();
      busylight.close();
    }, 3000);

  } catch (err) {
    console.error('Error:', err);
  }
})();
