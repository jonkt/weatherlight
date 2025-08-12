/**
 * @fileoverview A simple class for creating timed, repeating actions.
 * Used by the Busylight library to create pulse and blink animations.
 * This version ensures that the timer stops itself after the specified number of ticks.
 */

/**
 * Creates a Stepper instance.
 * @class
 * @param {number} ticks The total number of times the action should be performed.
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
 * Stops the timer if the end is reached.
 */
Stepper.prototype.tick = function() {
  // Stop the timer first if we've reached the end
  if (this.index >= this.ticks) {
    this.stop();
    return;
  }

  this.callback(this.index);
  this.index++;
};

/**
 * Stops the stepper and clears the timer.
 */
Stepper.prototype.stop = function() {
  if (this.timer) {
    clearInterval(this.timer);
    this.timer = null;
  }
};

module.exports = Stepper;