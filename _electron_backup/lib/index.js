const Busylight = require('./busylight');
const supported = require('../supported.json');

// The get method is the main entry point.
// It creates a new Busylight instance.
module.exports.get = function get() {
  return new Busylight(supported);
};