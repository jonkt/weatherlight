/**
 * @fileoverview A simple class for creating timed, repeating actions.
 * Used by the Busylight library to create continuous pulse and blink animations.
 */

/**
 * Creates a Stepper instance that loops continuously.
 * @class
 * @param {number} ticks The total number of ticks in one full cycle.
 * @param {number} rate The time in ms between each tick.
 * @param {function} callback The function to call on each tick, passed the current index.
 */
function Stepper(ticks, rate, callback) {
  this.ticks = ticks;
  this.rate = rate;
  this.callback = callback;
  this.index = 0;
  // Start the stepper immediately upon creation.
  this.timer = setInterval(this.tick.bind(this), this.rate);
}

/**
 * Executes a single step of the animation.
 * Calls the callback and increments the index.
 * If the end of the cycle is reached, it resets the index to 0 to loop.
 */
Stepper.prototype.tick = function() {
  this.callback(this.index);

  this.index++;
  // When the last tick is reached, reset to 0 for the next interval.
  if (this.index >= this.ticks) {
    this.index = 0;
  }
};

/**
 * Stops the stepper and clears the looping timer.
 * This is called when the light state needs to change (e.g., to solid or off).
 */
Stepper.prototype.stop = function() {
  if (this.timer) {
    clearInterval(this.timer);
    this.timer = null;
  }
};

module.exports = Stepper;