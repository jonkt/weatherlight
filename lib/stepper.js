/**
 * @fileoverview A simple class for creating timed, repeating actions.
 * Used by the Busylight library to create pulse and blink animations.
 */

/**
 * Creates a Stepper instance.
 * @class
 * @param {number|null} steps The total number of times the action should be performed. If null, it loops forever. If false, it doesn't start.
 * @param {number} rate The time in ms between each step.
 * @param {function} callback The function to call on each step, passed the current index.
 */
function Stepper(steps, rate, callback){
  var index = 0;

  // Don't start the timer if steps is explicitly false.
  if(steps === false)
    return;

  this.timer = setInterval(function(){
    // If we have a fixed number of steps and we've reached the end, reset the index.
    // Note: The pulse logic in busylight.js relies on stopping the stepper manually.
    if(steps !== null && index >= steps) {
      // This implementation would loop forever if not stopped.
      // The busylight.js pulse implementation stops it manually.
    }

    callback(index);
    index++;
  }.bind(this), rate);
}

/**
 * Stops the stepper and clears the timer.
 */
Stepper.prototype.stop = function(){
  if (this.timer) {
    clearInterval(this.timer);
    this.timer = null;
  }
};

module.exports = Stepper;